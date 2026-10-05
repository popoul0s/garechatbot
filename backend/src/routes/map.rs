//! Couches GeoJSON pour la carte.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

fn point(lon: f64, lat: f64, properties: Value) -> Value {
    json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [lon, lat] },
        "properties": properties,
    })
}

fn collection(features: Vec<Value>) -> Value {
    json!({ "type": "FeatureCollection", "features": features })
}

#[derive(Deserialize)]
pub struct StationsParams {
    /// Nom de la ville d'origine : ajoute le temps de trajet à chaque gare.
    origin: Option<String>,
}

/// GET /api/map/stations?origin=Grenoble
pub async fn stations(State(st): State<AppState>, Query(p): Query<StationsParams>) -> AppResult<Json<Value>> {
    let origin_id = match p.origin {
        Some(name) => db::resolve_station(&st.db, &name).await?.map(|s| s.id),
        None => None,
    };
    let rows = db::map_stations(&st.db, origin_id).await?;
    let features = rows
        .into_iter()
        .map(|r| {
            point(
                r.station.lon,
                r.station.lat,
                json!({
                    "id": r.station.id,
                    "name": r.station.name,
                    "city": r.station.city,
                    "pmr": r.station.pmr,
                    "minutes": r.minutes,
                    "nb_changes": r.nb_changes,
                    "poi_count": r.poi_count,
                    "is_origin": Some(r.station.id) == origin_id,
                }),
            )
        })
        .collect();
    Ok(Json(collection(features)))
}

/// GET /api/map/lines
pub async fn lines(State(st): State<AppState>) -> AppResult<Json<Value>> {
    let features = db::map_lines(&st.db)
        .await?
        .into_iter()
        .map(|l| {
            let geometry: Value = serde_json::from_str(&l.geojson).unwrap_or(Value::Null);
            json!({
                "type": "Feature",
                "geometry": geometry,
                "properties": { "id": l.id, "route_id": l.route_id, "name": l.name, "color": l.color },
            })
        })
        .collect();
    Ok(Json(collection(features)))
}

#[derive(Deserialize)]
pub struct PoisParams {
    /// "minLon,minLat,maxLon,maxLat"
    bbox: Option<String>,
    tag: Option<String>,
}

/// GET /api/map/pois?bbox=5.5,45.0,6.0,45.4&tag=nature
pub async fn pois(State(st): State<AppState>, Query(p): Query<PoisParams>) -> AppResult<Json<Value>> {
    let bbox = match p.bbox {
        Some(b) => {
            let parts: Vec<f64> = b.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let arr: [f64; 4] = parts
                .try_into()
                .map_err(|_| AppError::BadRequest("bbox attendu : minLon,minLat,maxLon,maxLat".into()))?;
            Some(arr)
        }
        None => None,
    };
    let features = db::map_pois(&st.db, bbox, p.tag.as_deref(), 3000)
        .await?
        .into_iter()
        .map(|poi| {
            point(
                poi.lon,
                poi.lat,
                json!({
                    "id": poi.id,
                    "name": poi.name,
                    "tags": poi.tags,
                    "source": poi.source,
                    "url": poi.url,
                    "description": poi.description,
                }),
            )
        })
        .collect();
    Ok(Json(collection(features)))
}
