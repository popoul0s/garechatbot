mod assistant;
mod map;
mod stations;

use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    let llm = state.llm.as_ref().map(|l| l.model().to_string());
    Router::new()
        .route("/api/health", get(move || async move { Json(json!({ "ok": true, "llm": llm })) }))
        .route("/api/stations", get(stations::search))
        .route("/api/stations/reachable", get(stations::reachable))
        .route("/api/stations/{id}", get(stations::detail))
        .route("/api/pois", get(stations::pois))
        .route("/api/map/stations", get(map::stations))
        .route("/api/map/lines", get(map::lines))
        .route("/api/map/pois", get(map::pois))
        .route("/api/search", post(assistant::search))
        .route("/api/chat", post(assistant::chat))
        .route("/api/chat/reset", post(assistant::reset))
        .with_state(state)
}
