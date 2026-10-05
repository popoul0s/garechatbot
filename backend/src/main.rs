mod config;
mod db;
mod domain;
mod error;
mod llm;
mod routes;
mod service;
mod state;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use sqlx::postgres::PgPoolOptions;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // .env à la racine du dépôt ou dans backend/
    dotenvy::from_filename("../.env").ok();
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()))
        .init();

    let cfg = Config::from_env()?;
    tracing::info!("connexion à la base de données…");
    let db = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&cfg.database_url)
        .await
        .context(
            "impossible de joindre PostgreSQL : Docker Desktop est-il lancé et `docker compose up -d db` exécuté ? \
             (vérifier aussi DATABASE_URL dans .env)",
        )?;
    let llm = cfg.llm.clone().map(llm::LlmClient::new).transpose()?;
    match &llm {
        Some(l) => tracing::info!(model = l.model(), "LLM configuré"),
        None => tracing::warn!("aucun LLM configuré : extraction par règles et réponses par gabarit"),
    }

    let state = AppState {
        db,
        cfg: Arc::new(cfg.clone()),
        llm,
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };
    let app = routes::router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!("API sur http://{}", cfg.bind_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
