//! Client minimal pour toute API compatible OpenAI (`POST {base}/chat/completions`).
//! Fonctionne avec Ollama (auto-hébergé), Mistral API, OpenAI, vLLM, LM Studio...
//! Un seul client pour les deux solutions comparées : seule la configuration change.

use std::time::Duration;

use anyhow::{anyhow, Context};
use serde_json::{json, Value};

use crate::config::LlmConfig;

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    cfg: LlmConfig,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

impl LlmClient {
    pub fn new(cfg: LlmConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.timeout_secs))
            .build()?;
        Ok(Self { http, cfg })
    }

    pub fn model(&self) -> &str {
        &self.cfg.model
    }

    /// Envoie un prompt système + utilisateur et renvoie un objet JSON parsé.
    pub async fn chat_json(&self, system: &str, user: &str) -> anyhow::Result<(Value, Usage)> {
        let body = json!({
            "model": self.cfg.model,
            "temperature": 0.1,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        });
        let mut req = self.http.post(format!("{}/chat/completions", self.cfg.base_url)).json(&body);
        if let Some(key) = &self.cfg.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().await.context("appel LLM")?;
        let status = resp.status();
        let payload: Value = resp.json().await.context("réponse LLM illisible")?;
        if !status.is_success() {
            return Err(anyhow!("LLM HTTP {status}: {payload}"));
        }
        let content = payload["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("réponse LLM sans contenu"))?;
        let usage = Usage {
            prompt_tokens: payload["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: payload["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        };
        Ok((parse_json_object(content)?, usage))
    }
}

/// Certains modèles entourent le JSON de texte ou de ```json ... ``` : on isole le premier objet.
pub fn parse_json_object(content: &str) -> anyhow::Result<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(content) {
        return Ok(v);
    }
    let start = content.find('{').ok_or_else(|| anyhow!("aucun JSON dans la réponse"))?;
    let end = content.rfind('}').ok_or_else(|| anyhow!("JSON incomplet"))?;
    serde_json::from_str(&content[start..=end]).context("JSON invalide")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_entoure_de_texte() {
        let v = parse_json_object("Voici :\n```json\n{\"a\": 1}\n```").unwrap();
        assert_eq!(v["a"], 1);
    }
}
