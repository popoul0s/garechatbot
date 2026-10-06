use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use sqlx::PgPool;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::config::Config;
use crate::domain::criteria::Criteria;
use crate::geo::GeoClient;
use crate::llm::LlmClient;
use crate::timetable::Timetable;

/// Contexte de conversation, conservé en mémoire le temps de la session (pas de persistance : hors périmètre).
#[derive(Debug, Clone)]
pub struct Session {
    pub criteria: Criteria,
    pub last_seen: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cfg: Arc<Config>,
    pub llm: Option<LlmClient>,
    pub geo: GeoClient,
    pub timetable: Arc<Timetable>,
    pub sessions: Arc<Mutex<HashMap<Uuid, Session>>>,
}
