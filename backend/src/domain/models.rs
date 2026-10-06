use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Station {
    pub id: i64,
    pub uic: String,
    pub name: String,
    pub city: Option<String>,
    pub lon: f64,
    pub lat: f64,
    pub pmr: Option<bool>,
    pub equipments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Poi {
    pub id: i64,
    pub source: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub url: Option<String>,
    pub lon: f64,
    pub lat: f64,
    /// Intérêt touristique (0..1) : 1 = site majeur (sommet, lac, château), 0.35 = square, aire de jeux.
    pub interest: f64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct PoiNearStation {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub poi: Poi,
    pub walk_minutes: i32,
    pub distance_m: i32,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReachableStation {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub station: Station,
    pub minutes: i32,
    pub nb_changes: i32,
    pub example_departure: Option<String>,
}

/// Une ligne (gare, POI) candidate renvoyée par la recherche SQL, avant classement.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CandidateRow {
    pub station_id: i64,
    pub station_name: String,
    pub station_city: Option<String>,
    pub station_lon: f64,
    pub station_lat: f64,
    pub station_pmr: Option<bool>,
    pub travel_minutes: Option<i32>,
    pub nb_changes: Option<i32>,
    pub example_departure: Option<String>,
    #[sqlx(flatten)]
    pub poi: Poi,
    pub walk_minutes: i32,
    pub distance_m: i32,
    pub text_rank: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StationSummary {
    pub id: i64,
    pub name: String,
    pub city: Option<String>,
    pub lon: f64,
    pub lat: f64,
    pub pmr: Option<bool>,
}

impl From<&Station> for StationSummary {
    fn from(s: &Station) -> Self {
        Self { id: s.id, name: s.name.clone(), city: s.city.clone(), lon: s.lon, lat: s.lat, pmr: s.pmr }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PoiHit {
    #[serde(flatten)]
    pub poi: Poi,
    pub walk_minutes: i32,
    pub distance_m: i32,
    /// Part des thèmes demandés couverts par ce POI (0..1).
    pub match_score: f64,
    /// Pertinence = correspondance x intérêt touristique : sert à ordonner les lieux.
    pub relevance: f64,
}

/// Détail du score : chaque composante est entre 0 et 1, avant pondération.
#[derive(Debug, Clone, Serialize)]
pub struct ScoreBreakdown {
    pub theme: f64,
    pub travel: f64,
    pub walk: f64,
    pub richness: f64,
    pub accessibility: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub station: StationSummary,
    pub travel_minutes: Option<i32>,
    pub nb_changes: Option<i32>,
    pub example_departure: Option<String>,
    pub score: f64,
    pub breakdown: ScoreBreakdown,
    pub pois: Vec<PoiHit>,
    /// Faits vérifiables produits par le code (jamais par le LLM).
    pub facts: Vec<String>,
}
