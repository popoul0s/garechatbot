//! Accès aux données (SQL + PostGIS). Aucune dépendance au LLM.

use sqlx::PgPool;

use crate::domain::criteria::Criteria;
use crate::domain::models::{CandidateRow, Poi, PoiNearStation, ReachableStation, Station, StationService};

const STATION_COLS: &str = "s.id, s.uic, s.name, s.city, s.lon, s.lat, s.pmr, s.equipments";
/// `interest` : intérêt touristique du lieu (0..1), dérivé de sa catégorie d'origine.
/// Un sommet, un lac ou un château valent plus qu'un square ou une aire de jeux.
/// `generic` : lieu OSM sans nom propre (nom par défaut donné à l'import, voir ingestion/osm.py).
const POI_COLS: &str = "p.id, p.source, p.name, p.description, p.tags, p.url, p.lon, p.lat,
    (p.source = 'osm' AND p.name IN ('Aire de jeux', 'Aire de pique-nique', 'Point de vue', 'Coin de pêche')) AS generic,
    (CASE
        WHEN p.source = 'osm' AND p.name IN ('Aire de jeux', 'Aire de pique-nique', 'Point de vue', 'Coin de pêche')
             THEN 0.35
        WHEN p.raw_types && ARRAY['natural=peak','natural=waterfall','natural=gorge','natural=cave_entrance',
             'water=lake','tourism=viewpoint','route=hiking','leisure=nature_reserve','leisure=fishing',
             'historic=castle','historic=abbey','historic=fort','historic=archaeological_site',
             'tourism=museum','tourism=zoo','tourism=theme_park'] THEN 1.0
        WHEN p.source = 'datatourisme' THEN 0.85
        WHEN p.raw_types && ARRAY['tourism=attraction','tourism=gallery','historic=monument','historic=ruins',
             'historic=manor','historic=city_gate','natural=beach','leisure=water_park','leisure=swimming_area',
             'leisure=garden'] THEN 0.7
        ELSE 0.35
     END)::float8 AS interest";

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
    // « gieres » trouve « Grenoble Universités Gières » : mot entier n'importe où dans le nom
    const WORDS: &str = "(' ' || regexp_replace(unaccent(lower(s.name)), '[^a-z0-9]+', ' ', 'g') || ' ')";
    const Q: &str = "trim(regexp_replace(unaccent(lower($1)), '[^a-z0-9]+', ' ', 'g'))";
    sqlx::query_as::<_, Station>(&format!(
        "SELECT {STATION_COLS} FROM stations s
         WHERE unaccent(lower(s.name)) LIKE unaccent(lower($1)) || '%'
            OR unaccent(lower(coalesce(s.city, ''))) = unaccent(lower($1))
            OR {WORDS} LIKE '% ' || {Q} || ' %'
            OR similarity(s.name, $1) > 0.4
         ORDER BY (unaccent(lower(s.name)) = unaccent(lower($1))) DESC,
                  (unaccent(lower(s.name)) LIKE unaccent(lower($1)) || '%') DESC,
                  ({WORDS} LIKE '% ' || {Q} || ' %') DESC,
                  EXISTS (SELECT 1 FROM travel_times t WHERE t.origin_id = s.id) DESC,
                  similarity(s.name, $1) DESC, length(s.name)
         LIMIT 1"
    ))
    .bind(name)
    .fetch_optional(db)
    .await
}

/// Table des services créée au démarrage : l'API fonctionne même si l'étape `services` n'a pas tourné.
pub async fn ensure_services_table(db: &PgPool) -> sqlx::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS station_services (
            station_id BIGINT NOT NULL REFERENCES stations(id) ON DELETE CASCADE,
            category TEXT NOT NULL, label TEXT NOT NULL, detail TEXT, source TEXT NOT NULL,
            PRIMARY KEY (station_id, category, source))",
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn station_services(db: &PgPool, station_id: i64) -> sqlx::Result<Vec<StationService>> {
    sqlx::query_as::<_, StationService>(
        "SELECT category, label, detail, source FROM station_services
         WHERE station_id = $1 AND category <> '_aucun'
         ORDER BY source = 'SNCF Open Data' DESC, category",
    )
    .bind(station_id)
    .fetch_all(db)
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

/// Gares pour lesquelles des temps de trajet ont été calculés (gares de départ possibles).
/// Gares proposées comme point de départ : toutes celles desservies par un train de la journée type
/// (les temps de trajet sont calculés à la demande). Sans horaires chargés : les origines pré-calculées.
pub async fn origins(db: &PgPool) -> sqlx::Result<Vec<Station>> {
    let has_timetable: bool =
        sqlx::query_scalar("SELECT to_regclass('connections') IS NOT NULL").fetch_one(db).await?;
    let served = if has_timetable {
        "EXISTS (SELECT 1 FROM connections c WHERE c.from_station = s.id)"
    } else {
        "EXISTS (SELECT 1 FROM travel_times t WHERE t.origin_id = s.id)"
    };
    sqlx::query_as::<_, Station>(&format!("SELECT {STATION_COLS} FROM stations s WHERE {served} ORDER BY s.name"))
        .fetch_all(db)
        .await
}

/// Enregistre les temps de trajet calculés depuis une nouvelle origine (cache pour les requêtes SQL).
pub async fn store_travel_times(
    db: &PgPool,
    origin_id: i64,
    times: &std::collections::HashMap<i64, crate::timetable::BestTime>,
) -> sqlx::Result<()> {
    let (mut ids, mut minutes, mut changes, mut deps) = (vec![], vec![], vec![], vec![]);
    // ordre stable des lignes insérées : deux insertions concurrentes verrouillent dans le même ordre
    let mut sorted: Vec<_> = times.iter().collect();
    sorted.sort_by_key(|(id, _)| **id);
    for (&id, t) in sorted {
        ids.push(id);
        minutes.push(t.minutes);
        changes.push(t.changes);
        deps.push(crate::timetable::hhmm(t.departure));
    }
    sqlx::query(
        "INSERT INTO travel_times (origin_id, station_id, minutes, nb_changes, example_departure)
         SELECT $1, * FROM UNNEST($2::bigint[], $3::int[], $4::int[], $5::text[])
         ON CONFLICT (origin_id, station_id) DO NOTHING",
    )
    .bind(origin_id)
    .bind(ids)
    .bind(minutes)
    .bind(changes)
    .bind(deps)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn has_travel_times(db: &PgPool, origin_id: i64) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM travel_times WHERE origin_id = $1)")
        .bind(origin_id)
        .fetch_one(db)
        .await
}

/// Noms de gares cités dans un texte libre, quelle que soit l'écriture (« a aix les bains » ->
/// « Aix-les-Bains - Le Revard »). Comparaison sur le nom de base (avant « - »), sans accents,
/// casse ni ponctuation. Renvoie (nom de base, clé normalisée), plus long d'abord.
pub async fn stations_in_text(db: &PgPool, text: &str) -> sqlx::Result<Vec<(String, String)>> {
    // Clés reconnues, de la plus sûre à la plus large :
    // 0. nom de base (« Aix-les-Bains ») ; 1. nom sans suffixe (« Gières Gare » -> « gieres ») ou commune ;
    // 2. mot distinctif du nom (« Grenoble Universités Gières » -> « gieres »).
    // Une même clé ne désigne qu'une gare : la plus sûre, puis le nom le plus court (« grenoble » -> Grenoble).
    sqlx::query_as(
        r#"WITH m AS (SELECT ' ' || regexp_replace(unaccent(lower($1)), '[^a-z0-9]+', ' ', 'g') || ' ' AS t),
                s AS (SELECT name, split_part(name, ' - ', 1) AS base, city,
                             trim(regexp_replace(unaccent(lower(split_part(name, ' - ', 1))), '[^a-z0-9]+', ' ', 'g')) AS bkey,
                             trim(regexp_replace(unaccent(lower(name)), '[^a-z0-9]+', ' ', 'g')) AS fkey
                      FROM stations),
                n AS (SELECT base AS label, bkey AS key, 0 AS prio, name FROM s
                      UNION ALL
                      SELECT base, regexp_replace(bkey, ' (gare|ville|centre|sncf)( .*)?$', ''), 1, name FROM s
                      UNION ALL
                      SELECT name, trim(regexp_replace(unaccent(lower(city)), '[^a-z0-9]+', ' ', 'g')), 1, name
                      FROM s WHERE city IS NOT NULL
                      UNION ALL
                      SELECT name, w, 2, name FROM s, regexp_split_to_table(fkey, ' ') AS w
                      WHERE length(w) >= 5 AND w NOT IN ('gare', 'ville', 'centre', 'universites', 'universite',
                            'saint', 'sainte', 'grand', 'grande', 'haute', 'basse', 'halte', 'route', 'riviere',
                            'plage', 'vieux', 'nord', 'ouest', 'champ', 'pont', 'lycee', 'campus', 'zone', 'chateau'))
           SELECT label, key FROM (
               SELECT DISTINCT ON (n.key) n.label, n.key
               FROM n, m
               WHERE length(n.key) >= 4 AND position(' ' || n.key || ' ' IN m.t) > 0
               ORDER BY n.key, n.prio, length(n.name)
           ) found
           ORDER BY length(key) DESC"#,
    )
    .bind(text)
    .fetch_all(db)
    .await
}

/// Gare dont le nom ou la commune correspond exactement au lieu demandé.
pub async fn station_named(db: &PgPool, place: &str) -> sqlx::Result<Option<Station>> {
    sqlx::query_as::<_, Station>(&format!(
        "SELECT {STATION_COLS} FROM stations s
         WHERE unaccent(lower(s.name)) = unaccent(lower($1))
            OR unaccent(lower(coalesce(s.city, ''))) = unaccent(lower($1))
            OR unaccent(lower(s.name)) LIKE unaccent(lower($1)) || ' %'
         ORDER BY length(s.name) LIMIT 1"
    ))
    .bind(place)
    .fetch_optional(db)
    .await
}

#[derive(sqlx::FromRow)]
pub struct NearStation {
    #[sqlx(flatten)]
    pub station: Station,
    pub distance_m: i32,
}

/// Gares les plus proches d'un point, dans un rayon donné.
pub async fn stations_near(db: &PgPool, lon: f64, lat: f64, max_m: f64, limit: i64) -> sqlx::Result<Vec<NearStation>> {
    sqlx::query_as::<_, NearStation>(&format!(
        "SELECT {STATION_COLS}, round(ST_Distance(s.geom, p.g))::int AS distance_m
         FROM stations s, (SELECT ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography AS g) p
         WHERE ST_DWithin(s.geom, p.g, $3)
         ORDER BY ST_Distance(s.geom, p.g) LIMIT $4"
    ))
    .bind(lon)
    .bind(lat)
    .bind(max_m)
    .bind(limit)
    .fetch_all(db)
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
    restrict_to: &[i64],
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
         WHERE CASE WHEN cardinality($5::bigint[]) > 0 THEN s.id = ANY($5)
                    ELSE (s.id = $1 OR t.minutes <= $2) END"
    ))
    .bind(origin_id)
    .bind(max_travel)
    .bind(max_walk)
    .bind(keywords)
    .bind(restrict_to)
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
           AND NOT (p.source = 'osm' AND p.name IN ('Aire de jeux', 'Aire de pique-nique', 'Point de vue', 'Coin de pêche'))
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
