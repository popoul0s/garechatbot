//! Itinéraires détaillés (quel train, à quelle heure, quelles correspondances).
//!
//! Les horaires théoriques de la journée type (table `connections`, remplie par l'ingestion GTFS)
//! sont chargés en mémoire au démarrage. Le calcul utilise le Connection Scan Algorithm (CSA) :
//! on parcourt les connexions par heure de départ croissante en retenant l'arrivée au plus tôt
//! à chaque gare, puis on remonte les trains empruntés pour reconstituer le trajet.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::PgPool;

/// Temps minimal de correspondance dans une même gare.
pub const MIN_TRANSFER_MIN: i32 = 5;
/// Durée maximale d'un trajet recherché.
const MAX_JOURNEY_MIN: i32 = 300;

#[derive(Debug, Clone, Copy)]
pub struct Conn {
    pub dep: i32,
    pub arr: i32,
    pub from: i64,
    pub to: i64,
    pub trip: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TripInfo {
    pub route_name: Option<String>,
    pub headsign: Option<String>,
    pub number: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StationInfo {
    pub name: String,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Timetable {
    conns: Vec<Conn>,
    trips: Vec<TripInfo>,
    stations: HashMap<i64, StationInfo>,
    /// Tracé réel des voies entre deux gares consécutives (clé : (min id, max id)), cf. ingestion/rail.py.
    rail: HashMap<(i64, i64), Vec<[f64; 2]>>,
    /// Journée type dont sont issus les horaires (AAAAMMJJ).
    pub service_date: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stop {
    pub station_id: i64,
    pub name: String,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Leg {
    pub from: Stop,
    pub to: Stop,
    pub departure: String,
    pub arrival: String,
    pub duration_min: i32,
    /// Attente en gare avant ce train (0 pour le premier).
    pub wait_before_min: i32,
    pub route_name: Option<String>,
    pub headsign: Option<String>,
    pub number: Option<String>,
    /// Gares desservies, départ et arrivée compris.
    pub stops: Vec<Stop>,
    /// Tracé à afficher : suit les voies quand leur géométrie est connue, sinon relie les gares.
    pub path: Vec<[f64; 2]>,
    #[serde(skip)]
    dep_min: i32,
    #[serde(skip)]
    arr_min: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Journey {
    pub departure: String,
    pub arrival: String,
    pub duration_min: i32,
    pub changes: usize,
    pub legs: Vec<Leg>,
    #[serde(skip)]
    dep_min: i32,
}

pub fn hhmm(min: i32) -> String {
    format!("{:02}:{:02}", (min / 60) % 24, min % 60)
}

impl Timetable {
    pub fn new(mut conns: Vec<Conn>, trips: Vec<TripInfo>, stations: HashMap<i64, StationInfo>) -> Self {
        conns.sort_by_key(|c| (c.dep, c.arr));
        Self { conns, trips, stations, rail: HashMap::new(), service_date: None }
    }

    /// Charge les horaires depuis la base. Tables absentes (ingestion pas encore relancée) => horaires vides.
    pub async fn load(db: &PgPool) -> anyhow::Result<Self> {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass('connections') IS NOT NULL").fetch_one(db).await?;
        if !exists {
            return Ok(Self::default());
        }
        type TripRow = (String, Option<String>, Option<String>, Option<String>);
        let trip_rows: Vec<TripRow> =
            sqlx::query_as("SELECT trip_id, route_name, headsign, number FROM gtfs_trips").fetch_all(db).await?;
        let mut trip_index = HashMap::new();
        let mut trips = Vec::with_capacity(trip_rows.len());
        for (id, route_name, headsign, number) in trip_rows {
            trip_index.insert(id, trips.len() as u32);
            trips.push(TripInfo { route_name, headsign, number });
        }
        let rows: Vec<(i32, i32, i64, i64, String)> =
            sqlx::query_as("SELECT dep_min, arr_min, from_station, to_station, trip_id FROM connections")
                .fetch_all(db)
                .await?;
        let conns = rows
            .into_iter()
            .filter_map(|(dep, arr, from, to, trip)| {
                trip_index.get(&trip).map(|&t| Conn { dep, arr, from, to, trip: t })
            })
            .collect();
        let stations: Vec<(i64, String, f64, f64)> =
            sqlx::query_as("SELECT id, name, lon, lat FROM stations").fetch_all(db).await?;
        let stations = stations
            .into_iter()
            .map(|(id, name, lon, lat)| (id, StationInfo { name, lon, lat }))
            .collect();
        let mut tt = Self::new(conns, trips, stations);
        let has_rail: bool =
            sqlx::query_scalar("SELECT to_regclass('rail_segments') IS NOT NULL").fetch_one(db).await?;
        if has_rail {
            let rows: Vec<(i64, i64, String)> =
                sqlx::query_as("SELECT from_station, to_station, ST_AsGeoJSON(geom, 5) FROM rail_segments")
                    .fetch_all(db)
                    .await?;
            for (a, b, geojson) in rows {
                let v: serde_json::Value = serde_json::from_str(&geojson)?;
                let coords: Vec<[f64; 2]> = serde_json::from_value(v["coordinates"].clone())?;
                tt.rail.insert((a.min(b), a.max(b)), if a <= b { coords } else { coords.into_iter().rev().collect() });
            }
        }
        tt.service_date = sqlx::query_scalar("SELECT value FROM gtfs_meta WHERE key = 'service_date'")
            .fetch_optional(db)
            .await
            .ok()
            .flatten();
        Ok(tt)
    }

    pub fn is_empty(&self) -> bool {
        self.conns.is_empty()
    }

    /// Tracé entre gares successives, orienté dans le sens du trajet.
    fn path_through(&self, stops: &[Stop]) -> Vec<[f64; 2]> {
        let mut out: Vec<[f64; 2]> = Vec::new();
        for w in stops.windows(2) {
            let (a, b) = (w[0].station_id, w[1].station_id);
            let seg: Vec<[f64; 2]> = match self.rail.get(&(a.min(b), a.max(b))) {
                Some(c) if a <= b => c.clone(),
                Some(c) => c.iter().rev().copied().collect(),
                None => vec![[w[0].lon, w[0].lat], [w[1].lon, w[1].lat]],
            };
            let skip = usize::from(!out.is_empty());
            out.extend(seg.into_iter().skip(skip));
        }
        out
    }

    fn stop(&self, id: i64) -> Stop {
        let s = self.stations.get(&id);
        Stop {
            station_id: id,
            name: s.map(|s| s.name.clone()).unwrap_or_else(|| format!("gare {id}")),
            lon: s.map_or(0.0, |s| s.lon),
            lat: s.map_or(0.0, |s| s.lat),
        }
    }

    /// Trajet arrivant le plus tôt à `to`, en partant de `from` au plus tôt à `start`.
    pub fn earliest(&self, from: i64, to: i64, start: i32) -> Option<Journey> {
        if from == to {
            return None;
        }
        // on cherche les départs jusqu'à 24 h plus tard ; c'est la durée du trajet qui est plafonnée
        let limit = start + 24 * 60;
        let first = self.conns.partition_point(|c| c.dep < start);
        let mut arrival: HashMap<i64, i32> = HashMap::from([(from, start)]);
        // gare -> (indice de la connexion où l'on est monté, indice de celle où l'on est descendu)
        let mut via: HashMap<i64, (usize, usize)> = HashMap::new();
        let mut boarded: HashMap<u32, usize> = HashMap::new();

        for (i, c) in self.conns.iter().enumerate().skip(first) {
            if c.dep > limit || arrival.get(&to).is_some_and(|&a| c.dep >= a) {
                break;
            }
            let enter = match boarded.get(&c.trip) {
                Some(&e) => e,
                None => {
                    let buffer = if c.from == from { 0 } else { MIN_TRANSFER_MIN };
                    match arrival.get(&c.from) {
                        Some(&a) if a + buffer <= c.dep => {
                            boarded.insert(c.trip, i);
                            i
                        }
                        _ => continue,
                    }
                }
            };
            if arrival.get(&c.to).is_none_or(|&a| c.arr < a) {
                arrival.insert(c.to, c.arr);
                via.insert(c.to, (enter, i));
            }
        }

        // reconstitution : on remonte de la gare d'arrivée jusqu'au départ
        let mut legs_idx = Vec::new();
        let mut at = to;
        while at != from {
            let &(enter, exit) = via.get(&at)?;
            legs_idx.push((enter, exit));
            at = self.conns[enter].from;
            if legs_idx.len() > 10 {
                return None;
            }
        }
        legs_idx.reverse();

        let mut legs: Vec<Leg> = Vec::new();
        for (enter, exit) in legs_idx {
            let (e, x) = (self.conns[enter], self.conns[exit]);
            let trip = self.trips.get(e.trip as usize).cloned().unwrap_or_default();
            // gares desservies : connexions successives du même train entre la montée et la descente
            let mut stops = vec![self.stop(e.from)];
            let mut cur = e.from;
            for c in &self.conns[enter..=exit] {
                if c.trip == e.trip && c.from == cur {
                    stops.push(self.stop(c.to));
                    cur = c.to;
                }
            }
            let wait = legs.last().map_or(0, |prev: &Leg| e.dep - prev.arr_min);
            let path = self.path_through(&stops);
            legs.push(Leg {
                from: self.stop(e.from),
                to: self.stop(x.to),
                departure: hhmm(e.dep),
                arrival: hhmm(x.arr),
                duration_min: x.arr - e.dep,
                wait_before_min: wait,
                route_name: trip.route_name,
                headsign: trip.headsign,
                number: trip.number,
                stops,
                path,
                dep_min: e.dep,
                arr_min: x.arr,
            });
        }
        let (dep, arr) = (legs.first()?.dep_min, arrival[&to]);
        if arr - dep > MAX_JOURNEY_MIN {
            return None;
        }
        Some(Journey {
            departure: hhmm(dep),
            arrival: hhmm(arr),
            duration_min: arr - dep,
            changes: legs.len() - 1,
            legs,
            dep_min: dep,
        })
    }

    /// Les `limit` prochains trajets partant après `after` (minutes depuis minuit).
    /// Un trajet qui part plus tôt mais arrive en même temps qu'un suivant est écarté.
    pub fn next_journeys(&self, from: i64, to: i64, after: i32, limit: usize) -> Vec<Journey> {
        let mut out: Vec<(i32, Journey)> = Vec::new();
        let mut start = after;
        for _ in 0..limit * 4 {
            let Some(j) = self.earliest(from, to, start) else { break };
            let dep = j.dep_min;
            if let Some(last) = out.last() {
                if last.1.arrival == j.arrival {
                    out.pop(); // partir plus tard pour arriver à la même heure est préférable
                }
            }
            out.push((dep, j));
            if out.len() > limit {
                break;
            }
            start = dep + 1;
        }
        out.truncate(limit);
        out.into_iter().map(|(_, j)| j).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A=Grenoble, B=correspondance, C=destination sur une autre ligne.
    fn tt() -> Timetable {
        let conns = vec![
            // train 0 : A 08:00 -> B 08:30 -> C' 09:00
            Conn { dep: 480, arr: 510, from: 1, to: 2, trip: 0 },
            Conn { dep: 511, arr: 540, from: 2, to: 4, trip: 0 },
            // train 1 : B 08:40 -> C 09:10 (correspondance de 10 min à B)
            Conn { dep: 520, arr: 550, from: 2, to: 3, trip: 1 },
            // train 2 : A 10:00 -> B 10:28
            Conn { dep: 600, arr: 628, from: 1, to: 2, trip: 2 },
            // train 3 : B 10:30 -> C 11:00 (correspondance de 2 min : trop courte)
            Conn { dep: 630, arr: 660, from: 2, to: 3, trip: 3 },
            // train 4 : B 11:00 -> C 11:30
            Conn { dep: 660, arr: 690, from: 2, to: 3, trip: 4 },
        ];
        let trips = (0..5)
            .map(|i| TripInfo { route_name: Some("TER".into()), headsign: None, number: Some(format!("1750{i}")) })
            .collect();
        let stations = [(1, "A"), (2, "B"), (3, "C"), (4, "D")]
            .into_iter()
            .map(|(id, n)| (id, StationInfo { name: n.into(), lon: 0.0, lat: 0.0 }))
            .collect();
        Timetable::new(conns, trips, stations)
    }

    #[test]
    fn trajet_avec_correspondance() {
        let j = tt().earliest(1, 3, 7 * 60).unwrap();
        assert_eq!(j.departure, "08:00");
        assert_eq!(j.arrival, "09:10");
        assert_eq!(j.changes, 1);
        assert_eq!(j.legs[0].number.as_deref(), Some("17500"));
        assert_eq!(j.legs[1].from.name, "B");
        assert_eq!(j.legs[1].wait_before_min, 10);
        assert_eq!(j.duration_min, 70);
    }

    #[test]
    fn correspondance_trop_courte_refusee() {
        // départ après 9h : train 2 arrive à B à 10:28, le 10:30 est trop juste (5 min mini) -> 11:00
        let j = tt().earliest(1, 3, 9 * 60).unwrap();
        assert_eq!(j.legs[1].departure, "11:00");
        assert_eq!(j.arrival, "11:30");
    }

    #[test]
    fn prochains_departs() {
        let js = tt().next_journeys(1, 3, 7 * 60, 3);
        let deps: Vec<_> = js.iter().map(|j| j.departure.as_str()).collect();
        assert_eq!(deps, vec!["08:00", "10:00"]);
        // gares desservies du premier train : A puis B
        assert_eq!(js[0].legs[0].stops.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
    }

    #[test]
    fn le_trace_suit_les_voies_dans_le_bon_sens() {
        let mut t = tt();
        // tracé connu B(2)->A(1) stocké sous la clé (1, 2) dans le sens 1 -> 2
        t.rail.insert((1, 2), vec![[0.0, 0.0], [0.5, 0.2], [1.0, 1.0]]);
        let j = t.earliest(1, 3, 7 * 60).unwrap();
        assert_eq!(j.legs[0].path, vec![[0.0, 0.0], [0.5, 0.2], [1.0, 1.0]]);
        // sans géométrie pour B->C : ligne droite entre les deux gares
        assert_eq!(j.legs[1].path.len(), 2);
        let stops = vec![t.stop(2), t.stop(1)];
        assert_eq!(t.path_through(&stops), vec![[1.0, 1.0], [0.5, 0.2], [0.0, 0.0]]);
    }

    #[test]
    fn longue_attente_avant_le_premier_train() {
        // à 5h du matin, le prochain train part à 8h : il doit être trouvé malgré 3 h d'attente
        let j = tt().earliest(1, 3, 2 * 60).unwrap();
        assert_eq!(j.departure, "08:00");
        assert_eq!(j.duration_min, 70, "la durée se compte depuis le départ du train, pas depuis l'heure demandée");
    }

    #[test]
    fn aucun_trajet() {
        assert!(tt().earliest(3, 1, 0).is_none());
    }
}
