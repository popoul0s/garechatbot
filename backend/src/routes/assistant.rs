//! /api/search (recherche structurée, sans IA) et /api/chat (langage naturel).

use std::time::Instant;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::criteria::{extract_with_rules, Criteria};
use crate::error::{AppError, AppResult};
use crate::llm::{self, Answer, Usage};
use crate::service::{self, SearchOutcome};
use crate::state::{AppState, Session};

/// POST /api/search — mêmes résultats que le chat, à partir de critères déjà structurés.
pub async fn search(State(st): State<AppState>, Json(criteria): Json<Criteria>) -> AppResult<Json<SearchOutcome>> {
    Ok(Json(service::search(&st, &criteria.sanitized()).await?))
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub session_id: Option<Uuid>,
    pub message: String,
    /// Gare sélectionnée sur la carte (contexte de la question). `null` = aucune.
    pub selected_station_id: Option<i64>,
    /// Critères actuellement affichés par l'interface (filtres éventuellement modifiés à la main).
    /// S'ils sont fournis, ils servent de contexte à la place de ceux mémorisés en session.
    pub context: Option<Criteria>,
}

#[derive(Serialize, Default)]
pub struct Engine {
    /// "llm" | "rules"
    pub extraction: &'static str,
    /// "llm" | "template"
    pub generation: &'static str,
    pub model: Option<String>,
    pub usage: Usage,
}

#[derive(Serialize, Default)]
pub struct Timings {
    pub extraction_ms: u128,
    pub search_ms: u128,
    pub generation_ms: u128,
    pub total_ms: u128,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub session_id: Uuid,
    /// Critères cumulés de la session, tels qu'appliqués à la recherche.
    pub criteria: Criteria,
    pub answer: Answer,
    #[serde(flatten)]
    pub outcome: SearchOutcome,
    pub engine: Engine,
    pub timings: Timings,
}

fn add_usage(total: &mut Usage, u: &Usage) {
    total.prompt_tokens += u.prompt_tokens;
    total.completion_tokens += u.completion_tokens;
}

/// POST /api/chat
pub async fn chat(State(st): State<AppState>, Json(req): Json<ChatRequest>) -> AppResult<Json<ChatResponse>> {
    let started = Instant::now();
    let message = req.message.trim();
    if message.is_empty() || message.len() > 1000 {
        return Err(AppError::BadRequest("message vide ou trop long".into()));
    }

    let session_id = req.session_id.unwrap_or_else(Uuid::new_v4);
    let previous = match req.context {
        Some(c) => c.sanitized(),
        None => st
            .sessions
            .lock()
            .await
            .get(&session_id)
            .map(|s| s.criteria.clone())
            .unwrap_or_default(),
    };

    let mut engine = Engine { model: st.llm.as_ref().map(|l| l.model().to_string()), ..Default::default() };
    let mut timings = Timings::default();

    // 1. Compréhension de la demande -> critères structurés
    let t = Instant::now();
    let extracted = match &st.llm {
        Some(llm) => match llm::extract_criteria(llm, message, &previous).await {
            Ok((c, usage)) => {
                engine.extraction = "llm";
                add_usage(&mut engine.usage, &usage);
                c
            }
            Err(e) => {
                tracing::warn!(error = %e, "extraction LLM en échec, repli sur les règles");
                engine.extraction = "rules";
                extract_with_rules(message)
            }
        },
        None => {
            engine.extraction = "rules";
            extract_with_rules(message)
        }
    };
    timings.extraction_ms = t.elapsed().as_millis();

    // 2. Fusion avec le contexte de session ; la gare sélectionnée sur la carte fait foi.
    let mut criteria = extracted.merged_onto(&previous);
    criteria.around_station_id = req.selected_station_id;

    // 3. Recherche et classement (déterministes)
    let t = Instant::now();
    let outcome = service::search(&st, &criteria).await?;
    timings.search_ms = t.elapsed().as_millis();

    // 4. Rédaction de la réponse à partir des seuls résultats
    let t = Instant::now();
    let answer = if outcome.recommendations.is_empty() {
        engine.generation = "template";
        Answer {
            intro: outcome
                .notes
                .last()
                .cloned()
                .unwrap_or_else(|| "Je n'ai trouvé aucune destination correspondante.".into()),
            items: vec![],
        }
    } else {
        match &st.llm {
            Some(llm) => match llm::write_answer(llm, message, &criteria, &outcome.recommendations).await {
                Ok((a, usage)) => {
                    engine.generation = "llm";
                    add_usage(&mut engine.usage, &usage);
                    a
                }
                Err(e) => {
                    tracing::warn!(error = %e, "rédaction LLM en échec, repli sur le gabarit");
                    engine.generation = "template";
                    llm::template_answer(&outcome.recommendations)
                }
            },
            None => {
                engine.generation = "template";
                llm::template_answer(&outcome.recommendations)
            }
        }
    };
    timings.generation_ms = t.elapsed().as_millis();

    {
        let mut sessions = st.sessions.lock().await;
        // ménage des sessions inactives depuis plus de 2 h
        sessions.retain(|_, s| s.last_seen.elapsed().as_secs() < 2 * 3600);
        sessions.insert(session_id, Session { criteria: criteria.clone(), last_seen: Instant::now() });
    }
    timings.total_ms = started.elapsed().as_millis();

    tracing::info!(
        %session_id,
        extraction = engine.extraction,
        generation = engine.generation,
        results = outcome.recommendations.len(),
        total_ms = timings.total_ms,
        "chat"
    );

    Ok(Json(ChatResponse { session_id, criteria, answer, outcome, engine, timings }))
}

/// POST /api/chat/reset — oublie les critères d'une session.
#[derive(Deserialize)]
pub struct ResetRequest {
    session_id: Uuid,
}

pub async fn reset(State(st): State<AppState>, Json(req): Json<ResetRequest>) -> AppResult<Json<serde_json::Value>> {
    st.sessions.lock().await.remove(&req.session_id);
    Ok(Json(serde_json::json!({ "ok": true })))
}
