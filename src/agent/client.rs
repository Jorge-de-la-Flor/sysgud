use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::json;

use crate::core::{ActionType, AgentAction, AgentRequest};

use super::prompt::{build_user_message, strip_code_fences, SYSTEM_PROMPT};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Cliente del motor de decisiones. Encapsula la llamada HTTP a la API
/// de Messages de Anthropic y el parseo de la respuesta estructurada.
pub struct AgentClient {
    http: reqwest::Client,
    api_key: Option<String>,
    model: String,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

impl AgentClient {
    pub fn new(api_key: Option<String>, model: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key,
            model,
        }
    }

    /// Envía el extracto de logs al LLM y devuelve la acción de
    /// remediación estructurada.
    ///
    /// Nunca propaga el error hacia arriba: si no hay API key configurada
    /// o la llamada falla por cualquier razón, degrada a una acción
    /// `NOTIFY` con el motivo en el diagnóstico. Esto evita que el daemon
    /// se caiga por una falla transitoria del motor de razonamiento.
    pub async fn analyze(&self, request: AgentRequest) -> Result<AgentAction> {
        let Some(api_key) = self.api_key.clone() else {
            return Ok(Self::fallback_action(
                "No se configuró ANTHROPIC_API_KEY; se omite el diagnóstico en vivo.",
            ));
        };

        match self.call_anthropic(&api_key, &request).await {
            Ok(action) => Ok(action),
            Err(err) => Ok(Self::fallback_action(&format!(
                "La llamada al agente falló, se usa NOTIFY por defecto: {err}"
            ))),
        }
    }

    async fn call_anthropic(&self, api_key: &str, request: &AgentRequest) -> Result<AgentAction> {
        let body = json!({
            "model": self.model,
            "max_tokens": 512,
            "system": SYSTEM_PROMPT,
            "messages": [
                { "role": "user", "content": build_user_message(request) }
            ]
        });

        let response = self
            .http
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<MessagesResponse>()
            .await?;

        let text = response
            .content
            .into_iter()
            .find(|block| block.kind == "text")
            .and_then(|block| block.text)
            .ok_or_else(|| anyhow!("la respuesta del agente no contenía un bloque de texto"))?;

        let clean = strip_code_fences(&text);
        let action: AgentAction = serde_json::from_str(clean)?;
        Ok(action)
    }

    fn fallback_action(diagnosis: &str) -> AgentAction {
        AgentAction {
            action_type: ActionType::Notify,
            command: None,
            diagnosis: diagnosis.to_string(),
        }
    }
}
