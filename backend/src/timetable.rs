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
/// Temps de trajet minimaux (mêmes règles que ingestion/gtfs.py) : départs entre 6 h et 20 h, 4 h de trajet au plus.
const TT_FIRST_DEPARTURE: i32 = 6 * 60;
const TT_LAST_DEPARTURE: i32 = 20 * 60;
const TT_MAX_TRAVEL: i32 = 240;

/// Meilleur temps de trajet vers une gare depuis une origine, sur la journée type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BestTime {
    pub minutes: i32,
    pub changes: i32,
    pub departure: i32,
}

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

    /// Durée minimale vers chaque gare depuis `origin`, en essayant chaque départ de la journée
    /// (CSA répété). Sert à proposer n'importe quelle gare comme point de départ.
    pub fn travel_times(&self, origin: i64) -> HashMap<i64, BestTime> {
        let mut departures: Vec<i32> = self
            .conns
            .iter()
            .filter(|c| c.from == origin && (TT_FIRST_DEPARTURE..=TT_LAST_DEPARTURE).contains(&c.dep))
            .map(|c| c.dep)
            .collect();
        departures.dedup();
        let mut best: HashMap<i64, BestTime> = HashMap::new();
        for start in departures {
            let limit = start + TT_MAX_TRAVEL;
            // gare -> (arrivée, nombre de trains)
            let mut arrival: HashMap<i64, (i32, i32)> = HashMap::from([(origin, (start, 0))]);
            let mut trip_legs: HashMap<u32, i32> = HashMap::new();
            let first = self.conns.partition_point(|c| c.dep < start);
            for c in &self.conns[first..] {
                if c.dep > limit {
                    break;
                }
                let n = match trip_legs.get(&c.trip) {
                    Some(&n) => n,
                    None => {
                        let buffer = if c.from == origin { 0 } else { MIN_TRANSFER_MIN };
                        match arrival.get(&c.from) {
                            Some(&(a, legs)) if a + buffer <= c.dep => {
                                trip_legs.insert(c.trip, legs + 1);
                                legs + 1
                            }
                            _ => continue,
                        }
                    }
                };
                let better = arrival.get(&c.to).is_none_or(|&(a, l)| c.arr < a || (c.arr == a && n < l));
                if better {
                    arrival.insert(c.to, (c.arr, n));
                }
            }
            for (station, (arr, n)) in arrival {
                if station == origin {
                    continue;
                }
                let t = BestTime { minutes: arr - start, changes: (n - 1).max(0), departure: start };
                if best.get(&station).is_none_or(|b| (t.minutes, t.changes) < (b.minutes, b.changes)) {
                    best.insert(station, t);
                }
            }
        }
        best
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
        Some(Journey {
            departure: hhmm(dep),
            arrival: hhmm(arr),
            duration_min: arr - dep,
            changes: legs.len() - 1,
            legs,
            dep_min: dep,
        })
    }

    /// Dernier trajet possible de la journée type partant après `after` (le « dernier train »).
    pub fn last_journey(&self, from: i64, to: i64, after: i32) -> Option<Journey> {
        let mut last = None;
        let mut start = after;
        // On enchaîne les départs successifs jusqu'à ce qu'aucun train n'atteigne plus la destination.
        // Un trajet trop long (longue attente en correspondance) est ignoré mais ne stoppe pas la recherche.
        for _ in 0..1000 {
            let Some(j) = self.earliest(from, to, start) else { break };
            if j.dep_min >= after + 24 * 60 {
                break;
            }
            start = j.dep_min + 1;
            if j.duration_min <= MAX_JOURNEY_MIN {
                last = Some(j);
            }
        }
        last
    }

    /// Les `limit` prochains trajets partant après `after` (minutes depuis minuit).
    /// Un trajet qui part plus tôt mais arrive en même temps qu'un suivant est écarté.
    pub fn next_journeys(&self, from: i64, to: i64, after: i32, limit: usize) -> Vec<Journey> {
        let mut out: Vec<(i32, Journey)> = Vec::new();
        let mut start = after;
        for _ in 0..limit * 20 {
            let Some(j) = self.earliest(from, to, start) else { break };
            let dep = j.dep_min;
            if j.duration_min > MAX_JOURNEY_MIN {
                // trajet aberrant (attente de plusieurs heures) : on cherche le départ suivant
                start = dep + 1;
                continue;
            }
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
    fn temps_de_trajet_depuis_n_importe_quelle_gare() {
        let t = tt().travel_times(1);
        assert_eq!(t[&2], BestTime { minutes: 28, changes: 0, departure: 600 });
        assert_eq!(t[&4], BestTime { minutes: 60, changes: 0, departure: 480 });
        assert_eq!(t[&3], BestTime { minutes: 70, changes: 1, departure: 480 });
        // depuis la gare de correspondance, C est en direct
        assert_eq!(tt().travel_times(2)[&3].changes, 0);
        assert!(!tt().travel_times(2).contains_key(&1));
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
    fn dernier_train_de_la_journee() {
        // A -> C : départs 08:00 et 10:00 ; le dernier est celui de 10:00, quelle que soit la limite d'affichage
        let j = tt().last_journey(1, 3, 7 * 60).unwrap();
        assert_eq!(j.departure, "10:00");
        assert!(tt().last_journey(1, 3, 11 * 60).is_none());
    }

    #[test]
    fn un_trajet_trop_long_n_arrete_pas_la_recherche() {
        // A->B 06:00, puis B->C seulement à 12:00 (6h30 au total, trop long) ; direct A->C à 18:00
        let conns = vec![
            Conn { dep: 360, arr: 390, from: 1, to: 2, trip: 0 },
            Conn { dep: 720, arr: 750, from: 2, to: 3, trip: 1 },
            Conn { dep: 1080, arr: 1140, from: 1, to: 3, trip: 2 },
        ];
        let trips = (0..3).map(|_| TripInfo::default()).collect();
        let stations = [(1, "A"), (2, "B"), (3, "C")]
            .into_iter()
            .map(|(id, n)| (id, StationInfo { name: n.into(), lon: 0.0, lat: 0.0 }))
            .collect();
        let t = Timetable::new(conns, trips, stations);
        assert_eq!(t.last_journey(1, 3, 5 * 60).unwrap().departure, "18:00");
        let deps: Vec<_> = t.next_journeys(1, 3, 5 * 60, 4).into_iter().map(|j| j.departure).collect();
        assert_eq!(deps, vec!["18:00"]);
    }

    #[test]
    fn aucun_trajet() {
        assert!(tt().earliest(3, 1, 0).is_none());
    }
}
