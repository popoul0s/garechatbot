//! Accès aux données (SQL + PostGIS). Aucune dépendance au LLM.

use sqlx::PgPool;

use crate::domain::criteria::Criteria;
use crate::domain::models::{CandidateRow, Poi, PoiNearStation, ReachableStation, Station};

const STATION_COLS: &str = "s.id, s.uic, s.name, s.city, s.lon, s.lat, s.pmr, s.equipments";
const POI_COLS: &str = "p.id, p.source, p.name, p.description, p.tags, p.url, p.lon, p.lat";

pub async fn search_stations(db: &PgPool, q: &str, limit: i64) -> sqlx::Result<Vec<Station>> {
    sqlx::query_as::<_, Station>(&format!(
        "SELECT {STATION_COLS} FROM stations s
         WHERE unaccent(lower(s.name)) LIKE '%' || unaccent(lower($1)) || '%'
            OR unaccent(lower(coalesce(s.city, ''))) LIKE '%' || unaccent(lower($1)) || '%'
            OR similarity(s.name, $1) > 0.3
         ORDER BY (unaccent(lower(s.name)) = unaccent(lower($1))) DESC, similarity(s.name, $1) DESC, s.name
         LIMIT $2"
    ))
    .bind(q)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Résout un nom de ville/gare en gare. Privilégie les gares qui servent d'origine
/// (temps de trajet pré-calculés), puis le nom exact, puis la similarité.
pub async fn resolve_station(db: &PgPool, name: &str) -> sqlx::Result<Option<Station>> {
    sqlx::query_as::<_, Station>(&format!(
        "SELECT {STATION_COLS} FROM stations s
         WHERE unaccent(lower(s.name)) LIKE unaccent(lower($1)) || '%'
            OR unaccent(lower(coalesce(s.city, ''))) = unaccent(lower($1))
            OR similarity(s.name, $1) > 0.4
         ORDER BY EXISTS (SELECT 1 FROM travel_times t WHERE t.origin_id = s.id) DESC,
                  (unaccent(lower(s.name)) = unaccent(lower($1))) DESC,
                  similarity(s.name, $1) DESC, length(s.name)
         LIMIT 1"
    ))
    .bind(name)
    .fetch_optional(db)
    .await
}

pub async fn get_station(db: &PgPool, id: i64) -> sqlx::Result<Option<Station>> {
    sqlx::query_as::<_, Station>(&format!("SELECT {STATION_COLS} FROM stations s WHERE s.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn pois_near_station(
    db: &PgPool,
    station_id: i64,
    max_walk: i32,
    tag: Option<&str>,
    limit: i64,
) -> sqlx::Result<Vec<PoiNearStation>> {
    sqlx::query_as::<_, PoiNearStation>(&format!(
        "SELECT {POI_COLS}, sp.walk_minutes, sp.distance_m
         FROM station_poi sp JOIN pois p ON p.id = sp.poi_id
         WHERE sp.station_id = $1 AND sp.walk_minutes <= $2 AND ($3::text IS NULL OR $3 = ANY(p.tags))
         ORDER BY sp.walk_minutes, p.name
         LIMIT $4"
    ))
    .bind(station_id)
    .bind(max_walk)
    .bind(tag)
    .bind(limit)
    .fetch_all(db)
    .await
}

pub async fn reachable_from(db: &PgPool, origin_id: i64, max_minutes: i32) -> sqlx::Result<Vec<ReachableStation>> {
    sqlx::query_as::<_, ReachableStation>(&format!(
        "SELECT {STATION_COLS}, t.minutes, t.nb_changes, t.example_departure
         FROM travel_times t JOIN stations s ON s.id = t.station_id
         WHERE t.origin_id = $1 AND t.minutes <= $2 AND t.station_id <> $1
         ORDER BY t.minutes"
    ))
    .bind(origin_id)
    .bind(max_minutes)
    .fetch_all(db)
    .await
}

pub async fn has_travel_times(db: &PgPool, origin_id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM travel_times WHERE origin_id = $1)")
        .bind(origin_id)
        .fetch_one(db)
        .await
}

/// Recherche structurée : toutes les paires (gare, POI) respectant les contraintes dures
/// (temps de trajet, temps de marche). Le classement est fait ensuite par `scoring::rank`.
pub async fn candidates(
    db: &PgPool,
    origin_id: i64,
    criteria: &Criteria,
    max_travel: i32,
    max_walk: i32,
) -> sqlx::Result<Vec<CandidateRow>> {
    let keywords = criteria.keywords.join(" or ");
    sqlx::query_as::<_, CandidateRow>(&format!(
        "SELECT s.id AS station_id, s.name AS station_name, s.city AS station_city,
                s.lon AS station_lon, s.lat AS station_lat, s.pmr AS station_pmr,
                CASE WHEN s.id = $1 THEN 0 ELSE t.minutes END AS travel_minutes,
                CASE WHEN s.id = $1 THEN 0 ELSE t.nb_changes END AS nb_changes,
                t.example_departure,
                {POI_COLS}, sp.walk_minutes, sp.distance_m,
                CASE WHEN $4 = '' THEN 0::real
                     ELSE ts_rank(p.tsv, websearch_to_tsquery('french', $4)) END AS text_rank
         FROM stations s
         LEFT JOIN travel_times t ON t.origin_id = $1 AND t.station_id = s.id
         JOIN station_poi sp ON sp.station_id = s.id AND sp.walk_minutes <= $3
         JOIN pois p ON p.id = sp.poi_id
         WHERE CASE WHEN $5::bigint IS NOT NULL THEN s.id = $5
                    ELSE s.id <> $1 AND t.minutes <= $2 END"
    ))
    .bind(origin_id)
    .bind(max_travel)
    .bind(max_walk)
    .bind(keywords)
    .bind(criteria.around_station_id)
    .fetch_all(db)
    .await
}

// ---------- Cartographie ----------

#[derive(sqlx::FromRow)]
pub struct MapStation {
    #[sqlx(flatten)]
    pub station: Station,
    pub minutes: Option<i32>,
    pub nb_changes: Option<i32>,
    pub poi_count: i64,
}

pub async fn map_stations(db: &PgPool, origin_id: Option<i64>) -> sqlx::Result<Vec<MapStation>> {
    sqlx::query_as::<_, MapStation>(&format!(
        "SELECT {STATION_COLS},
                CASE WHEN s.id = $1 THEN 0 ELSE t.minutes END AS minutes,
                CASE WHEN s.id = $1 THEN 0 ELSE t.nb_changes END AS nb_changes,
                (SELECT count(*) FROM station_poi sp WHERE sp.station_id = s.id) AS poi_count
         FROM stations s
         LEFT JOIN travel_times t ON t.origin_id = $1 AND t.station_id = s.id"
    ))
    .bind(origin_id)
    .fetch_all(db)
    .await
}

#[derive(sqlx::FromRow)]
pub struct MapLine {
    pub id: i32,
    pub route_id: String,
    pub name: String,
    pub color: Option<String>,
    pub geojson: String,
}

pub async fn map_lines(db: &PgPool) -> sqlx::Result<Vec<MapLine>> {
    sqlx::query_as::<_, MapLine>(
        "SELECT id, route_id, name, color, ST_AsGeoJSON(geom, 5) AS geojson FROM lines",
    )
    .fetch_all(db)
    .await
}

pub async fn map_pois(
    db: &PgPool,
    bbox: Option<[f64; 4]>,
    tag: Option<&str>,
    limit: i64,
) -> sqlx::Result<Vec<Poi>> {
    let [min_lon, min_lat, max_lon, max_lat] = bbox.unwrap_or([-180.0, -90.0, 180.0, 90.0]);
    sqlx::query_as::<_, Poi>(&format!(
        "SELECT {POI_COLS} FROM pois p
         WHERE p.lon BETWEEN $1 AND $3 AND p.lat BETWEEN $2 AND $4
           AND ($5::text IS NULL OR $5 = ANY(p.tags))
           AND EXISTS (SELECT 1 FROM station_poi sp WHERE sp.poi_id = p.id)
         LIMIT $6"
    ))
    .bind(min_lon)
    .bind(min_lat)
    .bind(max_lon)
    .bind(max_lat)
    .bind(tag)
    .bind(limit)
    .fetch_all(db)
    .await
}
