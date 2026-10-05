//! Logique métier de recherche : résolution de l'origine, requête, classement, assouplissement.
//! Utilisée à l'identique par `/api/search` (sans IA) et `/api/chat` (avec IA).

use serde::Serialize;

use crate::db;
use crate::domain::criteria::Criteria;
use crate::domain::models::{Recommendation, StationSummary};
use crate::domain::scoring::{self, format_minutes, DEFAULT_MAX_TRAVEL, DEFAULT_MAX_WALK};
use crate::error::AppResult;
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
}

pub async fn search(state: &AppState, criteria: &Criteria) -> AppResult<SearchOutcome> {
    let mut notes = Vec::new();

    let around = match criteria.around_station_id {
        Some(id) => db::get_station(&state.db, id).await?,
        None => None,
    };

    let origin_name = match &criteria.origin {
        Some(o) => o.clone(),
        None => {
            if around.is_none() {
                notes.push(format!("Ville de départ non précisée : {} par défaut.", state.cfg.default_origin));
            }
            state.cfg.default_origin.clone()
        }
    };
    let origin = db::resolve_station(&state.db, &origin_name).await?;

    let mut outcome = SearchOutcome {
        origin: origin.as_ref().map(StationSummary::from),
        around_station: around.as_ref().map(StationSummary::from),
        applied_max_travel_minutes: criteria.max_travel_minutes.unwrap_or(DEFAULT_MAX_TRAVEL),
        applied_max_walk_minutes: criteria.max_walk_minutes.unwrap_or(DEFAULT_MAX_WALK),
        relaxed: false,
        recommendations: Vec::new(),
        notes,
    };

    // Sans origine connue, on ne peut chercher qu'autour d'une gare sélectionnée.
    let origin_id = match (&origin, &around) {
        (Some(o), _) => o.id,
        (None, Some(a)) => a.id,
        (None, None) => {
            outcome.notes.push(format!(
                "Je ne trouve pas de gare correspondant à « {origin_name} » dans les données disponibles."
            ));
            return Ok(outcome);
        }
    };
    if around.is_none() && !db::has_travel_times(&state.db, origin_id).await? {
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
                "Aucune destination des données disponibles ne correspond à ces critères, même en les assouplissant. \
                 Essayez un autre thème ou une durée plus longue."
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

    Ok(outcome)
}

/// Rayon dans lequel on cherche une gare autour d'une commune sans gare.
const PLACE_RADIUS_M: f64 = 12_000.0;

enum PlaceResolution {
    /// Gares à utiliser, avec une explication éventuelle pour l'utilisateur.
    Stations(Vec<i64>, Option<String>),
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
    let list = near
        .iter()
        .map(|n| format!("{} ({:.1} km)", n.station.name, n.distance_m as f64 / 1000.0).replace('.', ","))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(PlaceResolution::Stations(
        near.iter().map(|n| n.station.id).collect(),
        Some(format!("{} n'a pas de gare : je cherche autour des gares les plus proches : {list}.", commune.nom)),
    ))
}
