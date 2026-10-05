use std::env;

#[derive(Clone, Debug)]
pub struct LlmConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: String,
    pub default_origin: String,
    pub geo_api_url: String,
    /// `None` => mode sans LLM (règles + gabarit).
    pub llm: Option<LlmConfig>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let non_empty = |key: &str| env::var(key).ok().filter(|v| !v.trim().is_empty());

        let llm = match (non_empty("LLM_BASE_URL"), non_empty("LLM_MODEL")) {
            (Some(base_url), Some(model)) => Some(LlmConfig {
                base_url: base_url.trim_end_matches('/').to_string(),
                model,
                api_key: non_empty("LLM_API_KEY"),
                timeout_secs: non_empty("LLM_TIMEOUT_SECS")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(60),
            }),
            _ => None,
        };

        Ok(Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://gare:gare@localhost:5432/garechatbot".into()),
            bind_addr: non_empty("BIND_ADDR").unwrap_or_else(|| "0.0.0.0:8080".into()),
            default_origin: non_empty("DEFAULT_ORIGIN").unwrap_or_else(|| "Grenoble".into()),
            geo_api_url: non_empty("GEO_API_URL").unwrap_or_else(|| "https://geo.api.gouv.fr".into()),
            llm,
        })
    }
}
