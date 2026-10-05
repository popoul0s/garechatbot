//! Localisation d'une commune par l'API Géo officielle (geo.api.gouv.fr, gratuite, sans clé).
//! Sert quand l'utilisateur nomme une destination sans gare ("pêcher à Herbeys").

use std::time::Duration;

use serde::Deserialize;

/// Départements d'Auvergne-Rhône-Alpes : on ne retient que les communes de la région.
const AURA_DEPARTEMENTS: &[&str] = &["01", "03", "07", "15", "26", "38", "42", "43", "63", "69", "73", "74"];

#[derive(Debug, Clone)]
pub struct Commune {
    pub nom: String,
    pub lon: f64,
    pub lat: f64,
}

#[derive(Deserialize)]
struct ApiCommune {
    nom: String,
    #[serde(rename = "codeDepartement")]
    code_departement: String,
    centre: Option<ApiPoint>,
}

#[derive(Deserialize)]
struct ApiPoint {
    coordinates: [f64; 2],
}

#[derive(Clone)]
pub struct GeoClient {
    http: reqwest::Client,
    base_url: String,
}

impl GeoClient {
    pub fn new(base_url: String) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(4)).build()?;
        Ok(Self { http, base_url: base_url.trim_end_matches('/').to_string() })
    }

    /// Commune d'AURA la plus peuplée portant ce nom.
    /// `Ok(None)` : aucune commune de la région ; `Err` : API injoignable.
    pub async fn find_commune(&self, name: &str) -> anyhow::Result<Option<Commune>> {
        let communes: Vec<ApiCommune> = self
            .http
            .get(format!("{}/communes", self.base_url))
            .query(&[("nom", name), ("fields", "nom,centre,codeDepartement"), ("boost", "population"), ("limit", "10")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(communes
            .into_iter()
            .filter(|c| AURA_DEPARTEMENTS.contains(&c.code_departement.as_str()))
            .find_map(|c| {
                let [lon, lat] = c.centre?.coordinates;
                Some(Commune { nom: c.nom, lon, lat })
            }))
    }
}
