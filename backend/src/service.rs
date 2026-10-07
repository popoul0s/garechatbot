//! Logique métier de recherche : résolution de l'origine, requête, classement, assouplissement.
//! Utilisée à l'identique par `/api/search` (sans IA) et `/api/chat` (avec IA).

use serde::Serialize;

use crate::db;
use crate::domain::criteria::Criteria;
use crate::domain::models::{Recommendation, StationSummary};
use crate::domain::scoring::{self, format_minutes, DEFAULT_MAX_TRAVEL, DEFAULT_MAX_WALK};
use crate::error::AppResult;
use crate::geo::Commune;
use crate::overpass;
use crate::state::AppState;

pub const MAX_RECOMMENDATIONS: usize = 5;

#[derive(Debug, Clone, Serialize)]
pub struct SearchOutcome {
    pub origin: Option<StationSummary>,
    pub around_station: Option<StationSummary>,
    /// Contraintes effectivement appliquées (après valeurs par défaut et éventuel assouplissement).
    pub applied_max_travel_minutes: i32,
    pub applied_max_walk_minutes: i32,
    pub relaxed: bool,
    pub recommendations: Vec<Recommendation>,
    /// Messages à afficher à l'utilisateur (valeurs par défaut, assouplissement, données manquantes).
    pub notes: Vec<String>,
    /// Gare de départ absente ou introuvable : l'interface doit la demander avant de chercher.
    pub needs_origin: bool,
    /// Destination sans gare : les lieux sont cherchés autour d'elle (et non autour des gares).
    pub place_area: Option<PlaceArea>,
}

/// Lieu demandé qui n'a pas de gare : centre de la commune et rayon de recherche des lieux.
#[derive(Debug, Clone, Serialize)]
pub struct PlaceArea {
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub radius_m: f64,
}

/// Rayon autour d'une commune sans gare dans lequel on cherche des lieux.
pub const AREA_RADIUS_M: f64 = 2_000.0;

/// Temps de trajet depuis une gare : calculés à la première demande à partir des horaires en mémoire,
/// puis gardés en base. Toute gare desservie peut ainsi servir de point de départ.
pub async fn ensure_travel_times(state: &AppState, origin_id: i64) -> AppResult<bool> {
    if db::has_travel_times(&state.db, origin_id).await? {
        return Ok(true);
    }
    // La carte et la recherche demandent souvent la même nouvelle origine en même temps :
    // un seul calcul à la fois, et on revérifie une fois le verrou obtenu (sinon : doublons, voire deadlock).
    static COMPUTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = COMPUTING.lock().await;
    if db::has_travel_times(&state.db, origin_id).await? {
        return Ok(true);
    }
    if state.timetable.is_empty() {
        return Ok(false);
    }
    let tt = state.timetable.clone();
    let times = tokio::task::spawn_blocking(move || tt.travel_times(origin_id))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    if times.is_empty() {
        return Ok(false);
    }
    db::store_travel_times(&state.db, origin_id, &times).await?;
    tracing::info!(origin_id, gares = times.len(), "temps de trajet calculés pour une nouvelle origine");
    Ok(true)
}

pub async fn search(state: &AppState, criteria: &Criteria) -> AppResult<SearchOutcome> {
    let around = match criteria.around_station_id {
        Some(id) => db::get_station(&state.db, id).await?,
        None => None,
    };

    // Pas de gare de départ par défaut : on la demande (sauf question sur une gare précise).
    let origin = match &criteria.origin {
        Some(o) => db::resolve_station(&state.db, o).await?,
        None => None,
    };

    let mut outcome = SearchOutcome {
        origin: origin.as_ref().map(StationSummary::from),
        around_station: around.as_ref().map(StationSummary::from),
        applied_max_travel_minutes: criteria.max_travel_minutes.unwrap_or(DEFAULT_MAX_TRAVEL),
        applied_max_walk_minutes: criteria.max_walk_minutes.unwrap_or(DEFAULT_MAX_WALK),
        relaxed: false,
        recommendations: Vec::new(),
        notes: Vec::new(),
        needs_origin: false,
        place_area: None,
    };

    // Sans origine connue, on ne peut chercher qu'autour d'une gare sélectionnée.
    let origin_id = match (&origin, &around) {
        (Some(o), _) => o.id,
        (None, Some(a)) => a.id,
        (None, None) => {
            outcome.needs_origin = true;
            outcome.notes.push(match &criteria.origin {
                Some(o) => format!("Je ne trouve pas de gare « {o} ». De quelle gare partez-vous ?"),
                None => "De quelle gare partez-vous ? Choisissez-la pour que je calcule les trajets.".into(),
            });
            return Ok(outcome);
        }
    };
    let origin_name = origin.as_ref().map_or_else(|| around.as_ref().map(|a| a.name.clone()).unwrap_or_default(), |o| o.name.clone());
    // les temps de trajet pré-calculés ne servent que pour une recherche « partout » (sans gare ni lieu imposés)
    if around.is_none() && criteria.place.is_none() && !ensure_travel_times(state, origin_id).await? {
        outcome.notes.push(format!(
            "Les temps de trajet depuis {origin_name} n'ont pas encore été calculés : je ne peux pas proposer de destination."
        ));
        return Ok(outcome);
    }

    // Gares auxquelles limiter la recherche : gare choisie sur la carte, ou gares proches du lieu demandé.
    let mut restrict: Vec<i64> = around.iter().map(|a| a.id).collect();
    if restrict.is_empty() {
        if let Some(place) = &criteria.place {
            match resolve_place(state, place).await? {
                PlaceResolution::Stations(ids, note) => {
                    restrict = ids;
                    outcome.notes.extend(note);
                }
                PlaceResolution::Area(commune, near) => {
                    return search_area(state, criteria, outcome, origin_id, commune, near).await;
                }
                PlaceResolution::NoStationNearby(note) => {
                    outcome.notes.push(note);
                    return Ok(outcome);
                }
                PlaceResolution::Unknown => {
                    outcome.notes.push(format!(
                        "Je ne trouve pas « {place} » parmi les gares et communes d'Auvergne-Rhône-Alpes. \
                         Vérifiez l'orthographe, ou retirez la destination pour chercher partout."
                    ));
                    return Ok(outcome);
                }
                PlaceResolution::GeoUnavailable => outcome.notes.push(format!(
                    "Impossible de localiser « {place} » pour le moment : je cherche dans toutes les gares accessibles."
                )),
            }
        }
    }

    let rows = db::candidates(
        &state.db,
        origin_id,
        criteria,
        outcome.applied_max_travel_minutes,
        outcome.applied_max_walk_minutes,
        &restrict,
    )
    .await?;
    outcome.recommendations = scoring::rank(rows, criteria, MAX_RECOMMENDATIONS);

    // Aucun résultat : on assouplit une fois les contraintes de durée, en le disant.
    if outcome.recommendations.is_empty() {
        let travel = (outcome.applied_max_travel_minutes + 30).min(240);
        let walk = (outcome.applied_max_walk_minutes + 15).min(60);
        let rows = db::candidates(&state.db, origin_id, criteria, travel, walk, &restrict).await?;
        let relaxed = scoring::rank(rows, criteria, MAX_RECOMMENDATIONS);
        if relaxed.is_empty() {
            outcome.notes.push(
                "Aucune destination des données disponibles ne correspond à ces critères, même en les assouplissant."
                    .into(),
            );
        } else {
            outcome.notes.push(format!(
                "Aucun résultat avec vos contraintes exactes : j'ai élargi à {} de train et {} min de marche.",
                format_minutes(travel),
                walk
            ));
            outcome.relaxed = true;
            outcome.applied_max_travel_minutes = travel;
            outcome.applied_max_walk_minutes = walk;
            outcome.recommendations = relaxed;
        }
    }

    // Question sur une gare précise : ses infos pratiques deviennent des faits vérifiables pour la réponse.
    if let Some(a) = &around {
        let facts = station_facts(state, a.id).await?;
        match outcome.recommendations.iter_mut().find(|r| r.station.id == a.id) {
            Some(r) => r.facts.extend(facts),
            None if !facts.is_empty() => {
                outcome.notes.push(format!("Infos pratiques de la gare {} : {}.", a.name, facts.join(" ; ")))
            }
            None => {}
        }
    }

    Ok(outcome)
}

/// Infos pratiques d'une gare sous forme de faits (trafic GTFS + services SNCF / OSM).
pub async fn station_facts(state: &AppState, station_id: i64) -> AppResult<Vec<String>> {
    let mut facts = Vec::new();
    if let Some(t) = state.timetable.station_traffic(station_id) {
        facts.push(format!(
            "{} trains au départ par jour (premier {}, dernier {}), {} gares en direct",
            t.departures,
            t.first_departure.unwrap_or_default(),
            t.last_departure.unwrap_or_default(),
            t.direct_destinations
        ));
    }
    for s in db::station_services(&state.db, station_id).await? {
        facts.push(match s.detail {
            Some(d) => format!("{} : {d} (source {})", s.label, s.source),
            None => format!("{} (source {})", s.label, s.source),
        });
    }
    Ok(facts)
}

/// Destination sans gare (« une balade à Herbeys ») : les lieux sont pris autour de la commune
/// elle-même, chargés depuis OpenStreetMap si la zone n'a jamais été vue, puis rattachés aux gares
/// d'accès les plus proches avec la distance réelle du dernier trajet.
async fn search_area(
    state: &AppState,
    criteria: &Criteria,
    mut outcome: SearchOutcome,
    origin_id: i64,
    commune: Commune,
    near: Vec<db::NearStation>,
) -> AppResult<SearchOutcome> {
    if let Err(e) = overpass::ensure_area(&state.db, commune.lon, commune.lat, AREA_RADIUS_M).await {
        tracing::warn!(error = %e, commune = %commune.nom, "lieux OSM de la zone non chargés");
    }
    // temps de train vers les gares d'accès (calculés à la demande si besoin)
    ensure_travel_times(state, origin_id).await?;
    let ids: Vec<i64> = near.iter().map(|n| n.station.id).collect();
    let rows =
        db::candidates_around_point(&state.db, origin_id, criteria, &ids, commune.lon, commune.lat, AREA_RADIUS_M)
            .await?;
    // pas de limite de marche ici : la note de marche est relative au lieu le plus éloigné
    let max_walk = rows.iter().map(|r| r.walk_minutes).max().unwrap_or(DEFAULT_MAX_WALK);
    let scoring_criteria = Criteria { max_walk_minutes: Some(max_walk), ..criteria.clone() };
    outcome.recommendations = scoring::rank(rows, &scoring_criteria, MAX_RECOMMENDATIONS);
    outcome.applied_max_walk_minutes = max_walk;

    let access = near
        .iter()
        .map(|n| format!("{} à {:.1} km", n.station.name, n.distance_m as f64 / 1000.0).replace('.', ","))
        .collect::<Vec<_>>()
        .join(", ");
    outcome.notes.push(if outcome.recommendations.is_empty() {
        format!(
            "{} n'a pas de gare, et je ne trouve aucun lieu correspondant dans un rayon de {} km. Gares les plus proches : {access}.",
            commune.nom,
            AREA_RADIUS_M as i32 / 1000
        )
    } else {
        format!(
            "{} n'a pas de gare : voici les lieux autour de {}, avec la gare d'accès la plus pratique ({access}). \
             Le dernier trajet se fait à pied, à vélo ou en bus.",
            commune.nom, commune.nom
        )
    });
    outcome.place_area = Some(PlaceArea { name: commune.nom, lon: commune.lon, lat: commune.lat, radius_m: AREA_RADIUS_M });
    Ok(outcome)
}

/// Rayon dans lequel on cherche une gare autour d'une commune sans gare.
const PLACE_RADIUS_M: f64 = 12_000.0;

enum PlaceResolution {
    /// Gares à utiliser, avec une explication éventuelle pour l'utilisateur.
    Stations(Vec<i64>, Option<String>),
    /// Commune sans gare : on cherche autour d'elle, avec ses gares d'accès les plus proches.
    Area(Commune, Vec<db::NearStation>),
    NoStationNearby(String),
    Unknown,
    GeoUnavailable,
}

/// "Herbeys" -> gare du même nom si elle existe, sinon gares les plus proches de la commune (API Géo).
async fn resolve_place(state: &AppState, place: &str) -> AppResult<PlaceResolution> {
    if let Some(s) = db::station_named(&state.db, place).await? {
        return Ok(PlaceResolution::Stations(vec![s.id], None));
    }
    let commune = match state.geo.find_commune(place).await {
        Ok(Some(c)) => c,
        Ok(None) => return Ok(PlaceResolution::Unknown),
        Err(e) => {
            tracing::warn!(error = %e, "API Géo injoignable");
            return Ok(PlaceResolution::GeoUnavailable);
        }
    };
    let near = db::stations_near(&state.db, commune.lon, commune.lat, PLACE_RADIUS_M, 3).await?;
    if near.is_empty() {
        return Ok(PlaceResolution::NoStationNearby(format!(
            "{} n'a pas de gare à moins de {} km : ce lieu n'est pas accessible en train dans nos données.",
            commune.nom,
            PLACE_RADIUS_M as i32 / 1000
        )));
    }
    Ok(PlaceResolution::Area(commune, near))
}
