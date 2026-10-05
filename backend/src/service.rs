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

    let rows = db::candidates(
        &state.db,
        origin_id,
        criteria,
        outcome.applied_max_travel_minutes,
        outcome.applied_max_walk_minutes,
    )
    .await?;
    outcome.recommendations = scoring::rank(rows, criteria, MAX_RECOMMENDATIONS);

    // Aucun résultat : on assouplit une fois les contraintes de durée, en le disant.
    if outcome.recommendations.is_empty() {
        let travel = (outcome.applied_max_travel_minutes + 30).min(240);
        let walk = (outcome.applied_max_walk_minutes + 15).min(60);
        let rows = db::candidates(&state.db, origin_id, criteria, travel, walk).await?;
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
