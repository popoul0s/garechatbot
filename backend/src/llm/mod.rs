//! Les deux seuls rôles du LLM :
//! 1. transformer la question en critères structurés (`extract_criteria`) ;
//! 2. rédiger une explication à partir des recommandations déjà choisies par le code (`write_answer`).
//!
//! Il ne choisit jamais les destinations et ne voit que les faits issus de la base.

pub mod client;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::LazyLock;

pub use client::{LlmClient, Usage};

use crate::domain::criteria::{Criteria, THEMES};
use crate::domain::models::Recommendation;

const EXTRACTION_PROMPT: &str = r#"Tu extrais les critères d'une demande de sortie touristique en train en Auvergne-Rhône-Alpes.
Réponds UNIQUEMENT avec un objet JSON de la forme :
{
  "origin": string|null,              // ville ou gare de départ explicitement citée
  "max_travel_minutes": int|null,     // durée maximale de TRAIN, en minutes ("1h30" -> 90)
  "themes": [string],                 // parmi : THEMES
  "audience": "famille"|null,         // "famille" si enfants ou famille mentionnés
  "max_walk_minutes": int|null,       // marche maximale depuis la gare, en minutes
  "difficulty": "facile"|"moyen"|"difficile"|null,
  "keywords": [string],               // mots-clés concrets utiles à une recherche texte (ex. "lac", "château", "pêche"), 0 à 4
  "place": string|null                // lieu de DESTINATION nommé (commune, gare, site), ex. "pêcher à Herbeys" -> "Herbeys"
}
Règles :
- Ne renseigne que ce qui est exprimé dans le DERNIER message ; mets null ou [] sinon.
- Les critères précédents de la conversation sont fournis pour comprendre les références implicites
  ("et si plutôt culturel ?" -> themes = ["culture"], le reste null).
- N'invente aucune ville ni durée."#;

const ANSWER_PROMPT: &str = r#"Tu es l'assistant d'une plateforme de tourisme en train en Auvergne-Rhône-Alpes.
On te fournit la demande de l'utilisateur et des recommandations DÉJÀ sélectionnées et classées par le système.
Rédige en français :
- "intro" : une phrase qui résume ce qui a été trouvé ;
- "items" : pour CHAQUE recommandation, dans le même ordre, {"station_id": <id fourni>, "explanation": "<1 à 2 phrases>"}.
Règles strictes :
- Utilise UNIQUEMENT les faits fournis (noms, durées, distances). N'ajoute aucun lieu, horaire, prix ou chiffre.
- N'ajoute pas de recommandation et n'en retire pas.
- Explique en quoi la destination correspond à la demande.
Réponds UNIQUEMENT avec un objet JSON {"intro": string, "items": [...]}."#;

pub async fn extract_criteria(
    llm: &LlmClient,
    message: &str,
    previous: &Criteria,
) -> anyhow::Result<(Criteria, Usage)> {
    let system = EXTRACTION_PROMPT.replace("THEMES", &THEMES.join(", "));
    let user = format!(
        "Critères précédents : {}\nDernier message : {}",
        serde_json::to_string(previous)?,
        message
    );
    let (value, usage) = llm.chat_json(&system, &user).await?;
    let criteria: Criteria = serde_json::from_value(value)?;
    Ok((criteria.sanitized(), usage))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerItem {
    pub station_id: i64,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub intro: String,
    pub items: Vec<AnswerItem>,
}

pub async fn write_answer(
    llm: &LlmClient,
    message: &str,
    criteria: &Criteria,
    recos: &[Recommendation],
) -> anyhow::Result<(Answer, Usage)> {
    let context: Vec<_> = recos
        .iter()
        .map(|r| {
            json!({
                "station_id": r.station.id,
                "gare": r.station.name,
                "faits": r.facts,
            })
        })
        .collect();
    let user = format!(
        "Demande : {message}\nCritères compris : {}\nRecommandations : {}",
        serde_json::to_string(criteria)?,
        serde_json::to_string(&context)?
    );
    let (value, usage) = llm.chat_json(ANSWER_PROMPT, &user).await?;
    let answer: Answer = serde_json::from_value(value)?;
    Ok((validate_answer(answer, recos), usage))
}

static RE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// Garde-fou anti-hallucination sur la sortie du LLM :
/// - on ne garde que les items dont l'id fait partie des recommandations ;
/// - une explication contenant un nombre absent des faits (horaire, durée inventés...)
///   est remplacée par l'explication générée par le code ;
/// - une recommandation oubliée par le LLM reçoit l'explication du code.
pub fn validate_answer(answer: Answer, recos: &[Recommendation]) -> Answer {
    let items = recos
        .iter()
        .map(|r| {
            let llm_text = answer
                .items
                .iter()
                .find(|i| i.station_id == r.station.id)
                .map(|i| i.explanation.trim().to_string())
                .filter(|t| !t.is_empty());
            let facts = format!("{} {}", r.station.name, r.facts.join(" "));
            let explanation = match llm_text {
                Some(text) if RE_NUMBER.find_iter(&text).all(|n| facts.contains(n.as_str())) => text,
                _ => template_explanation(r),
            };
            AnswerItem { station_id: r.station.id, explanation }
        })
        .collect();
    Answer { intro: answer.intro, items }
}

pub fn template_explanation(r: &Recommendation) -> String {
    r.facts.join(". ") + "."
}

/// Réponse sans LLM : construite uniquement à partir des faits.
pub fn template_answer(recos: &[Recommendation]) -> Answer {
    Answer {
        intro: format!(
            "Voici {} destination{} accessible{} en train correspondant à votre demande :",
            recos.len(),
            if recos.len() > 1 { "s" } else { "" },
            if recos.len() > 1 { "s" } else { "" }
        ),
        items: recos
            .iter()
            .map(|r| AnswerItem { station_id: r.station.id, explanation: template_explanation(r) })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{ScoreBreakdown, StationSummary};

    fn reco(id: i64) -> Recommendation {
        Recommendation {
            station: StationSummary { id, name: format!("Gare {id}"), city: None, lon: 0.0, lat: 0.0, pmr: None },
            travel_minutes: Some(45),
            nb_changes: Some(0),
            example_departure: None,
            score: 0.8,
            breakdown: ScoreBreakdown { theme: 1.0, travel: 0.5, walk: 0.5, richness: 0.2, accessibility: 0.5 },
            pois: vec![],
            facts: vec!["Trajet en train : 45 min (direct)".into(), "Lac Bleu à 10 min à pied de la gare".into()],
        }
    }

    #[test]
    fn retire_les_destinations_inventees_et_les_chiffres_non_sources() {
        let answer = Answer {
            intro: "ok".into(),
            items: vec![
                AnswerItem { station_id: 1, explanation: "À 45 min en train, le Lac Bleu est à 10 min.".into() },
                AnswerItem { station_id: 2, explanation: "Train toutes les 30 min, départ 7h12.".into() },
                AnswerItem { station_id: 999, explanation: "Destination inventée".into() },
            ],
        };
        let out = validate_answer(answer, &[reco(1), reco(2)]);
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.items[0].explanation, "À 45 min en train, le Lac Bleu est à 10 min.");
        assert!(out.items[1].explanation.starts_with("Trajet en train : 45 min"), "chiffres inventés => gabarit");
        assert!(out.items.iter().all(|i| i.station_id != 999));
    }
}
