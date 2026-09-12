use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

use crate::core::SysgudError;

/// URL base de la API de Telegram Bot.
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

/// Cliente de Telegram para envío de mensajes `sendMessage`
/// y polling de `getUpdates`.
///
/// Reutiliza el mismo patrón de `AgentClient`: un `reqwest::Client`
/// propio, token y chat_id configurados, y degradación a consola
/// ante cualquier fallo de red.
#[derive(Clone)]
pub struct TelegramClient {
    http: Client,
    pub token: String,
    pub chat_id: String,
}

/// Respuesta de la API `sendMessage`.
#[derive(Deserialize)]
struct SendMessageResponse {
    ok: bool,
    description: Option<String>,
}

/// Respuesta de la API `getUpdates`.
#[derive(Deserialize)]
pub struct GetUpdatesResponse {
    pub ok: bool,
    pub result: Vec<Update>,
}

/// Una actualización de Telegram.
#[derive(Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
}

/// Un mensaje de Telegram.
#[derive(Deserialize)]
pub struct Message {
    pub from: From,
    pub text: Option<String>,
}

/// El remitente de un mensaje de Telegram.
#[derive(Deserialize)]
pub struct From {
    pub id: i64,
}

impl TelegramClient {
    /// Crea un nuevo cliente con el token y chat_id proporcionados.
    pub fn new(token: String, chat_id: String) -> Self {
        Self {
            http: Client::new(),
            token,
            chat_id,
        }
    }

    /// Retorna la URL de `sendMessage` construida a partir del token.
    /// Exponible para testing.
    pub fn build_url(&self) -> String {
        format!("{}{}/sendMessage", TELEGRAM_API_BASE, self.token)
    }

    /// Envía un mensaje de diagnóstico al chat de Telegram.
    ///
    /// POST a `https://api.telegram.org/bot<token>/sendMessage` con
    /// `chat_id` y `text`. Cualquier error de red, timeout, código
    /// 4xx/5xx o 429 (rate limit) se convierte en `Ok(())` tras
    /// imprimir una advertencia en consola: el sistema NUNCA debe
    /// propagar errores de Telegram al llamador.
    pub async fn send(&self, text: &str) -> Result<(), SysgudError> {
        let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, self.token);

        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
        });

        match self.http.post(&url).json(&body).send().await {
            Ok(response) => {
                let parsed: SendMessageResponse = response.json().await.map_err(|e| {
                    SysgudError::Telegram(format!("failed to parse sendMessage response: {e}"))
                })?;
                if !parsed.ok {
                    let desc = parsed.description.unwrap_or_default();
                    println!("[telegram] sendMessage returned error: {}", desc);
                    return Ok(());
                }
                println!("[telegram] message sent to chat {}", self.chat_id);
                Ok(())
            }
            Err(e) => {
                println!("[telegram] sendMessage failed, degrading to console: {e}");
                Ok(())
            }
        }
    }

    /// Realiza un GET a `getUpdates` con el offset y timeout dados.
    pub async fn get_updates(&self, url: &str) -> Result<GetUpdatesResponse> {
        let response = self.http.get(url).send().await?;
        let parsed: GetUpdatesResponse = response.json().await?;
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_uses_correct_base_and_token() {
        let client = TelegramClient::new("my-token-123".to_string(), "999".to_string());
        assert_eq!(
            client.build_url(),
            "https://api.telegram.org/botmy-token-123/sendMessage"
        );
    }

    #[test]
    fn build_url_different_tokens_produce_different_urls() {
        let c1 = TelegramClient::new("token-a".to_string(), "1".to_string());
        let c2 = TelegramClient::new("token-b".to_string(), "2".to_string());
        assert_ne!(c1.build_url(), c2.build_url());
    }
}