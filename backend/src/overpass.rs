//! Lieux OpenStreetMap chargés à la demande autour d'un lieu sans gare (« une balade à Herbeys »).
//!
//! L'ingestion (ingestion/osm.py) ne récupère les lieux qu'à 3 km des gares : un village sans gare
//! n'a donc souvent aucun lieu en base. Ce module interroge l'API Overpass au moment de la recherche,
//! avec les mêmes filtres et la même traduction des étiquettes, puis garde le résultat en base.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;

const OVERPASS_URLS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];
const USER_AGENT: &str = "Aiguillage/0.1 (projet etudiant tourisme ferroviaire AURA)";

/// Mêmes filtres que ingestion/osm.py (les lacs y sont pris par leur centre : la zone est petite).
const FILTERS: &[&str] = &[
    r#"["tourism"~"^(attraction|museum|viewpoint|zoo|theme_park|picnic_site|gallery)$"]"#,
    r#"["historic"~"^(castle|monument|ruins|archaeological_site|fort|abbey|city_gate|manor)$"]"#,
    r#"["leisure"~"^(park|nature_reserve|playground|water_park|swimming_area|garden)$"]"#,
    r#"["natural"~"^(peak|waterfall|cave_entrance|beach|gorge)$"]"#,
    r#"["leisure"="fishing"]"#,
    r#"["water"="lake"]["name"]"#,
];

#[derive(Deserialize)]
struct Response {
    elements: Vec<Element>,
}

#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    lat: Option<f64>,
    lon: Option<f64>,
    center: Option<Center>,
    #[serde(default)]
    tags: HashMap<String, String>,
}

#[derive(Deserialize)]
struct Center {
    lat: f64,
    lon: f64,
}

/// Étiquettes OSM -> thèmes de l'application (identique à `map_tags` de ingestion/osm.py).
pub fn map_tags(t: &HashMap<String, String>) -> Vec<String> {
    let get = |k: &str| t.get(k).map(String::as_str);
    let (tourism, historic, leisure, natural) = (get("tourism"), get("historic"), get("leisure"), get("natural"));
    let mut out: BTreeSet<&str> = BTreeSet::new();
    let mut add = |v: &[&'static str]| out.extend(v.iter().copied());
    match tourism {
        Some("museum") => add(&["musee", "culture"]),
        Some("gallery") => add(&["culture"]),
        Some("attraction") => add(&["loisirs"]),
        Some("viewpoint") => add(&["panorama", "nature"]),
        Some("zoo" | "theme_park") => add(&["loisirs", "famille"]),
        Some("picnic_site") => add(&["nature", "famille"]),
        _ => {}
    }
    if historic.is_some() {
        add(&["patrimoine", "culture"]);
    }
    match leisure {
        Some("park" | "garden") => add(&["nature", "famille"]),
        Some("nature_reserve") => add(&["nature"]),
        Some("playground") => add(&["famille", "loisirs"]),
        Some("water_park" | "swimming_area") => add(&["eau", "loisirs", "famille"]),
        Some("fishing") => add(&["eau", "nature"]),
        _ => {}
    }
    match natural {
        Some("peak") => add(&["montagne", "panorama", "randonnee", "nature"]),
        Some("waterfall" | "gorge") => add(&["nature", "eau"]),
        Some("cave_entrance") => add(&["nature"]),
        Some("beach") => add(&["eau", "nature"]),
        _ => {}
    }
    if get("water") == Some("lake") {
        add(&["eau", "nature"]);
    }
    if get("ele").and_then(|e| e.replace(',', ".").parse::<f64>().ok()).is_some_and(|e| e >= 1000.0) {
        add(&["montagne"]);
    }
    out.into_iter().map(String::from).collect()
}

/// Nom affiché : le nom OSM, ou un nom générique pour les lieux qui n'en ont pas (comme osm.py).
pub fn default_name(t: &HashMap<String, String>) -> Option<String> {
    if let Some(n) = t.get("name").filter(|n| !n.trim().is_empty()) {
        return Some(n.clone());
    }
    let generic = match (t.get("leisure").map(String::as_str), t.get("tourism").map(String::as_str)) {
        (Some("playground"), _) => "Aire de jeux",
        (_, Some("picnic_site")) => "Aire de pique-nique",
        (_, Some("viewpoint")) => "Point de vue",
        (Some("fishing"), _) => "Coin de pêche",
        _ => return None,
    };
    Some(generic.into())
}

fn description(t: &HashMap<String, String>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = t.get("description:fr").or_else(|| t.get("description")) {
        parts.push(d.clone());
    }
    if t.get("leisure").map(String::as_str) == Some("fishing") {
        parts.push("Lieu de pêche".into());
    }
    if let Some(e) = t.get("ele") {
        parts.push(format!("Altitude {e} m"));
    }
    (!parts.is_empty()).then(|| parts.join(". "))
}

/// Charge (une seule fois par zone) les lieux OSM dans un rayon autour d'un point.
/// Renvoie le nombre de lieux ajoutés ; une erreur réseau n'est pas fatale pour la recherche.
pub async fn ensure_area(db: &PgPool, lon: f64, lat: f64, radius_m: f64) -> anyhow::Result<usize> {
    sqlx::query("CREATE TABLE IF NOT EXISTS osm_area_done (area TEXT PRIMARY KEY, fetched_at TIMESTAMPTZ DEFAULT now())")
        .execute(db)
        .await?;
    // une zone = le point arrondi à ~1 km : deux recherches sur la même commune ne refont pas l'appel
    let area = format!("{:.2},{:.2},{}", lat, lon, radius_m as i64);
    let done: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM osm_area_done WHERE area = $1)")
        .bind(&area)
        .fetch_one(db)
        .await?;
    if done {
        return Ok(0);
    }

    let dlat = radius_m / 111_000.0;
    let dlon = radius_m / (111_000.0 * lat.to_radians().cos());
    let bbox = format!("({:.5},{:.5},{:.5},{:.5})", lat - dlat, lon - dlon, lat + dlat, lon + dlon);
    let body: String = FILTERS.iter().map(|f| format!("nwr{f}{bbox};")).collect();
    let query = format!("[out:json][timeout:25];({body});out center tags;");

    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).user_agent(USER_AGENT).build()?;
    let mut last_err = None;
    let mut elements = None;
    for url in OVERPASS_URLS {
        match http.post(*url).form(&[("data", query.as_str())]).send().await {
            Ok(r) if r.status().is_success() => match r.json::<Response>().await {
                Ok(resp) => {
                    elements = Some(resp.elements);
                    break;
                }
                Err(e) => last_err = Some(anyhow::anyhow!(e)),
            },
            Ok(r) => last_err = Some(anyhow::anyhow!("{url} : HTTP {}", r.status())),
            Err(e) => last_err = Some(anyhow::anyhow!(e)),
        }
    }
    let Some(elements) = elements else {
        return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Overpass indisponible")));
    };

    let mut added = 0;
    for e in elements {
        let (Some(lon), Some(lat)) = (e.lon.or(e.center.as_ref().map(|c| c.lon)), e.lat.or(e.center.as_ref().map(|c| c.lat)))
        else {
            continue;
        };
        let tags = map_tags(&e.tags);
        let Some(name) = default_name(&e.tags) else { continue };
        if tags.is_empty() {
            continue;
        }
        let raw: Vec<String> = ["tourism", "historic", "leisure", "natural", "water", "route"]
            .iter()
            .filter_map(|k| e.tags.get(*k).map(|v| format!("{k}={v}")))
            .collect();
        let res = sqlx::query(
            "INSERT INTO pois (source, source_id, name, description, tags, raw_types, url, lon, lat, geom, ele)
             VALUES ('osm', $1, $2, $3, $4, $5, $6, $7, $8, ST_SetSRID(ST_MakePoint($7, $8), 4326)::geography, $9)
             ON CONFLICT (source, source_id) DO NOTHING",
        )
        .bind(format!("{}/{}", e.kind, e.id))
        .bind(&name)
        .bind(description(&e.tags))
        .bind(&tags)
        .bind(&raw)
        .bind(e.tags.get("website"))
        .bind(lon)
        .bind(lat)
        .bind(e.tags.get("ele").and_then(|v| v.replace(',', ".").trim_end_matches('m').trim().parse::<f32>().ok()))
        .execute(db)
        .await?;
        added += res.rows_affected() as usize;
    }
    // les nouveaux lieux proches d'une gare servent aussi aux recherches classiques (comme link.py)
    sqlx::query(
        "INSERT INTO station_poi (station_id, poi_id, distance_m, walk_minutes, climb_m)
         SELECT s.id, p.id, round(ST_Distance(s.geom, p.geom))::int,
                greatest(1, ceil(ST_Distance(s.geom, p.geom) * 1.3 / (5000.0 / 60)
                                 + greatest(0, coalesce(p.ele - s.ele, 0)) * 0.1))::int,
                CASE WHEN p.ele IS NOT NULL AND s.ele IS NOT NULL THEN greatest(0, round(p.ele - s.ele))::int END
         FROM pois p JOIN stations s ON ST_DWithin(s.geom, p.geom, 3000)
         WHERE ST_DWithin(p.geom, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(lon)
    .bind(lat)
    .bind(radius_m)
    .execute(db)
    .await?;
    sqlx::query("INSERT INTO osm_area_done (area) VALUES ($1) ON CONFLICT DO NOTHING").bind(&area).execute(db).await?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn traduction_des_etiquettes_comme_l_ingestion() {
        assert_eq!(map_tags(&tags(&[("water", "lake"), ("name", "Lac")])), vec!["eau", "nature"]);
        assert_eq!(map_tags(&tags(&[("natural", "peak"), ("ele", "1450")])), vec!["montagne", "nature", "panorama", "randonnee"]);
        assert_eq!(map_tags(&tags(&[("historic", "castle")])), vec!["culture", "patrimoine"]);
        assert_eq!(default_name(&tags(&[("leisure", "fishing")])).as_deref(), Some("Coin de pêche"));
        assert_eq!(default_name(&tags(&[("leisure", "park")])), None);
    }
}
