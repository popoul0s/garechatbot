//! GET /api/journeys : prochains trains entre deux gares (horaires théoriques de la journée type).

use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::timetable::Journey;

#[derive(Deserialize)]
pub struct Params {
    from_id: i64,
    to_id: i64,
    /// "HH:MM" : partir après cette heure (08:00 par défaut)
    after: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub struct Response {
    journeys: Vec<Journey>,
    /// Journée type des horaires, au format JJ/MM/AAAA
    service_date: Option<String>,
    /// Avertissement à afficher (horaires théoriques, absence de données...)
    note: String,
}

fn parse_hhmm(s: &str) -> Option<i32> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    ((0..48).contains(&h) && (0..60).contains(&m)).then_some(h * 60 + m)
}

/// GET /api/journeys?from_id=1&to_id=42&after=08:00&limit=4
pub async fn journeys(State(st): State<AppState>, Query(p): Query<Params>) -> AppResult<Json<Response>> {
    let after = match p.after.as_deref() {
        Some(s) => parse_hhmm(s).ok_or_else(|| AppError::BadRequest("after attendu au format HH:MM".into()))?,
        None => 8 * 60,
    };
    let tt = &st.timetable;
    let service_date = tt
        .service_date
        .as_deref()
        .filter(|d| d.len() == 8)
        .map(|d| format!("{}/{}/{}", &d[6..8], &d[4..6], &d[0..4]));
    let note = if tt.is_empty() {
        "Horaires détaillés indisponibles : relancer l'ingestion GTFS.".to_string()
    } else {
        format!(
            "Horaires théoriques SNCF d'un jour de semaine type ({}). À vérifier avant de partir (travaux, grèves, week-end).",
            service_date.as_deref().unwrap_or("date inconnue")
        )
    };
    let journeys = tt.next_journeys(p.from_id, p.to_id, after, p.limit.unwrap_or(4).min(10));
    Ok(Json(Response { journeys, service_date, note }))
}
