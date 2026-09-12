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
    redactor: crate::security::Redactor,
    api_url: String,
    request_timeout: std::time::Duration,
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
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("HTTP client"),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
            model,
            redactor: crate::security::Redactor::from_env(),
            api_url: ANTHROPIC_API_URL.to_owned(),
            request_timeout: std::time::Duration::from_secs(30),
        }
    }

    /// Envía el extracto de logs al LLM y devuelve la acción de
    /// remediación estructurada.
    ///
    /// Nunca propaga el error hacia arriba: si no hay API key configurada
    /// o la llamada falla por cualquier razón, degrada a una acción
    /// `NOTIFY` con el motivo en el diagnóstico. Esto evita que el daemon
    /// se caiga por una falla transitoria del motor de razonamiento.
    pub async fn analyze(&self, mut request: AgentRequest) -> Result<AgentAction> {
        let redactor = &self.redactor;
        request.system_context = redactor.redact(&request.system_context);
        request.log_extract = request
            .log_extract
            .iter()
            .take(100)
            .map(|line| {
                if line.len() > 8192 {
                    "[LÍNEA OMITIDA POR TAMAÑO]".to_owned()
                } else {
                    redactor.redact(line)
                }
            })
            .collect();
        let Some(api_key) = self.api_key.clone() else {
            return Ok(Self::fallback_action(
                "No se configuró ANTHROPIC_API_KEY; se omite el diagnóstico en vivo.",
            ));
        };

        match self.call_anthropic(&api_key, &request).await {
            Ok(action) => Ok(action),
            Err(_) => Ok(Self::fallback_action(
                "La llamada al agente falló; se usa NOTIFY por defecto. Compruebe configuración y conectividad."
            )),
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

        let mut response = self
            .http
            .post(&self.api_url)
            .timeout(self.request_timeout)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        const MAX_RESPONSE: usize = 64 * 1024;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(
                bytes.len() + chunk.len() <= MAX_RESPONSE,
                "respuesta LLM demasiado grande"
            );
            bytes.extend_from_slice(&chunk);
        }
        let response: MessagesResponse = serde_json::from_slice(&bytes)?;
        let text = response
            .content
            .into_iter()
            .find(|block| block.kind == "text")
            .and_then(|block| block.text)
            .ok_or_else(|| anyhow!("la respuesta del agente no contenía un bloque de texto"))?;

        let clean = strip_code_fences(&text);
        let mut action: AgentAction = serde_json::from_str(clean)?;
        anyhow::ensure!(
            !action.diagnosis.trim().is_empty() && action.diagnosis.len() <= 4096,
            "diagnóstico inválido"
        );
        anyhow::ensure!(
            action.command.as_ref().is_none_or(|v| v.len() <= 4096),
            "comando demasiado grande"
        );
        anyhow::ensure!(
            action.action_type == ActionType::Execute || action.command.is_none(),
            "comando inesperado"
        );
        action.diagnosis = self.redactor.redact(&action.diagnosis);
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn local_client(
        status: &str,
        body: String,
        stall: bool,
    ) -> (AgentClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = AgentClient::new(Some("test-key".into()), "test".into());
        client.api_url = format!("http://{}", listener.local_addr().unwrap());
        client.request_timeout = std::time::Duration::from_millis(100);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 8192];
            let _ = socket.read(&mut request).await;
            if stall {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            let _ = socket.write_all(response.as_bytes()).await;
        });
        (client, task)
    }

    fn request() -> AgentRequest {
        AgentRequest {
            system_context: "test".into(),
            log_extract: vec!["ERROR test".into()],
        }
    }

    #[tokio::test]
    async fn network_and_parse_failures_fall_back_without_side_effects() {
        for (status, body, stall) in [
            ("500 Internal Server Error", "{}".into(), false),
            ("200 OK", "not json".into(), false),
            ("200 OK", "x".repeat(70_000), false),
            ("200 OK", "{}".into(), true),
        ] {
            let (client, server) = local_client(status, body, stall).await;
            let action = client.analyze(request()).await.unwrap();
            assert_eq!(action.action_type, ActionType::Notify);
            assert!(action.command.is_none());
            assert!(action.diagnosis.contains("falló"));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn parses_a_valid_proposal_from_mock_http() {
        let body = serde_json::json!({"content":[{"type":"text","text":"{\"action_type\":\"NOTIFY\",\"command\":null,\"diagnosis\":\"diagnosis from mock\"}"}]}).to_string();
        let (client, server) = local_client("200 OK", body, false).await;
        let action = client.analyze(request()).await.unwrap();
        assert_eq!(action.action_type, ActionType::Notify);
        assert_eq!(action.diagnosis, "diagnosis from mock");
        server.await.unwrap();
    }
}
