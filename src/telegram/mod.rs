//! Módulo de integración con Telegram para notificaciones salientes
//! y comandos entrantes.
//!
//! Proporciona:
//! - [`client`]: envío de `sendMessage` y `getUpdates` a través de la API de Telegram.
//! - [`commands`]: parseo de comandos entrantes (`/status`, `/approve`, `/reject`).
//! - [`updates`]: loop de polling `getUpdates` concurrente con el monitor.
//!
//! El módulo está deshabilitado por defecto: si `TELEGRAM_BOT_TOKEN` y
//! `TELEGRAM_CHAT_ID` no están configuradas, [`is_enabled()`] retorna `false`
//! y todo el flujo de Telegram se omite manteniendo el comportamiento
//! de consola únicamente.

pub mod client;
pub mod commands;
pub mod updates;

use crate::core::Config;
use tokio::sync::Mutex;
use std::sync::Arc;

/// Determina si el módulo de Telegram está habilitado.
///
/// Retorna `true` solo cuando tanto `TELEGRAM_BOT_TOKEN` como
/// `TELEGRAM_CHAT_ID` están presentes en la configuración.
pub fn is_enabled(config: &Config) -> bool {
    config.telegram_bot_token.is_some() && config.telegram_chat_id.is_some()
}

/// Contexto compartido del bot de Telegram entre el loop de polling
/// y el ejecutor de acciones.
pub struct TelegramCtx {
    /// Cliente HTTP opcional: `None` cuando Telegram está deshabilitado.
    pub client: Option<client::TelegramClient>,
    /// Aprobación pendiente que aguarda confirmación desde chat.
    pub pending: Arc<Mutex<Option<crate::core::PendingApproval>>>,
    /// Lista de IDs de remitentes permitidos para comandos.
    pub allowlist: Vec<String>,
}

impl TelegramCtx {
    /// Crea un contexto con los datos de configuración.
    ///
    /// Si `is_enabled()` retorna `false`, `client` será `None` y el
    /// polling no se lanzará.
    pub fn new(config: &Config) -> Self {
        let client = if is_enabled(config) {
            Some(client::TelegramClient::new(
                config.telegram_bot_token.clone().unwrap(),
                config.telegram_chat_id.clone().unwrap(),
            ))
        } else {
            None
        };
        Self {
            client,
            pending: Arc::new(Mutex::new(None)),
            allowlist: config.telegram_allowlist.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Config;

    #[test]
    fn is_enabled_returns_true_when_both_set() {
        let config = Config {
            api_key: None,
            model: "test".to_string(),
            context_lines: 12,
            target_program: "test".to_string(),
            target_args: vec![],
            telegram_bot_token: Some("token".to_string()),
            telegram_chat_id: Some("chat".to_string()),
            telegram_allowlist: vec![],
        };
        assert!(is_enabled(&config));
    }

    #[test]
    fn is_enabled_returns_false_when_token_missing() {
        let config = Config {
            api_key: None,
            model: "test".to_string(),
            context_lines: 12,
            target_program: "test".to_string(),
            target_args: vec![],
            telegram_bot_token: None,
            telegram_chat_id: Some("chat".to_string()),
            telegram_allowlist: vec![],
        };
        assert!(!is_enabled(&config));
    }

    #[test]
    fn is_enabled_returns_false_when_chat_id_missing() {
        let config = Config {
            api_key: None,
            model: "test".to_string(),
            context_lines: 12,
            target_program: "test".to_string(),
            target_args: vec![],
            telegram_bot_token: Some("token".to_string()),
            telegram_chat_id: None,
            telegram_allowlist: vec![],
        };
        assert!(!is_enabled(&config));
    }
}