use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::db;
use crate::domain::models::{PoiNearStation, ReachableStation, Station};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Deserialize)]
pub struct SearchParams {
    q: String,
    limit: Option<i64>,
}

/// GET /api/stations?q=gren
pub async fn search(State(st): State<AppState>, Query(p): Query<SearchParams>) -> AppResult<Json<Vec<Station>>> {
    Ok(Json(db::search_stations(&st.db, &p.q, p.limit.unwrap_or(10).min(50)).await?))
}

#[derive(Deserialize)]
pub struct DetailParams {
    max_walk: Option<i32>,
}

#[derive(Serialize)]
pub struct StationDetail {
    station: Station,
    pois: Vec<PoiNearStation>,
}

/// GET /api/stations/{id}?max_walk=30
pub async fn detail(
    State(st): State<AppState>,
    Path(id): Path<i64>,
    Query(p): Query<DetailParams>,
) -> AppResult<Json<StationDetail>> {
    let station = db::get_station(&st.db, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("gare {id}")))?;
    let pois = db::pois_near_station(&st.db, id, p.max_walk.unwrap_or(30), None, 200).await?;
    Ok(Json(StationDetail { station, pois }))
}

#[derive(Deserialize)]
pub struct ReachableParams {
    from: String,
    max_minutes: Option<i32>,
}

#[derive(Serialize)]
pub struct ReachableResponse {
    origin: Station,
    stations: Vec<ReachableStation>,
}

/// GET /api/stations/reachable?from=Grenoble&max_minutes=90
pub async fn reachable(
    State(st): State<AppState>,
    Query(p): Query<ReachableParams>,
) -> AppResult<Json<ReachableResponse>> {
    let origin = db::resolve_station(&st.db, &p.from)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("gare « {} »", p.from)))?;
    let stations = db::reachable_from(&st.db, origin.id, p.max_minutes.unwrap_or(120)).await?;
    Ok(Json(ReachableResponse { origin, stations }))
}

#[derive(Deserialize)]
pub struct PoiParams {
    station_id: i64,
    max_walk: Option<i32>,
    tag: Option<String>,
}

/// GET /api/pois?station_id=12&max_walk=20&tag=randonnee
pub async fn pois(State(st): State<AppState>, Query(p): Query<PoiParams>) -> AppResult<Json<Vec<PoiNearStation>>> {
    Ok(Json(
        db::pois_near_station(&st.db, p.station_id, p.max_walk.unwrap_or(30), p.tag.as_deref(), 200).await?,
    ))
}
