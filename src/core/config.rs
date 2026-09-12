use std::env;

/// Configuración de ejecución de sysgud, resuelta desde variables de entorno.
///
/// Todas las variables son opcionales y tienen un valor por defecto que
/// reproduce la demo original (un script de Python que revienta con OOM).
#[derive(Debug, Clone)]
pub struct Config {
    /// `ANTHROPIC_API_KEY`. Si no está presente, el agente cae en modo
    /// degradado (siempre `NOTIFY`) en lugar de fallar.
    pub api_key: Option<String>,
    /// `SYSGUD_MODEL`. Modelo a invocar en la API de Messages.
    pub model: String,
    /// `SYSGUD_CONTEXT_LINES`. Tamaño del buffer circular de logs.
    pub context_lines: usize,
    /// `SYSGUD_TARGET_CMD`. Binario del proceso a supervisar.
    pub target_program: String,
    /// `SYSGUD_TARGET_ARGS`. Argumentos separados por espacio para el proceso objetivo.
    pub target_args: Vec<String>,
    /// `TELEGRAM_BOT_TOKEN`. Token del bot de Telegram para notificaciones.
    pub telegram_bot_token: Option<String>,
    /// `TELEGRAM_CHAT_ID`. Chat ID de destino para las notificaciones.
    pub telegram_chat_id: Option<String>,
    /// `TELEGRAM_ALLOWLIST`. IDs de remitentes permitidos (CSV).
    pub telegram_allowlist: Vec<String>,
}

impl Config {
    /// Construye la configuración a partir del entorno del proceso.
    pub fn from_env() -> Self {
        let target_program =
            env::var("SYSGUD_TARGET_CMD").unwrap_or_else(|_| "python3".to_string());

        let target_args_raw = env::var("SYSGUD_TARGET_ARGS").unwrap_or_default();
        let target_args = if target_args_raw.trim().is_empty() {
            default_demo_args()
        } else {
            target_args_raw
                .split(' ')
                .map(str::to_string)
                .collect::<Vec<_>>()
        };

        let telegram_bot_token = env::var("TELEGRAM_BOT_TOKEN").ok();
        let telegram_chat_id = env::var("TELEGRAM_CHAT_ID").ok();
        let telegram_allowlist = env::var("TELEGRAM_ALLOWLIST")
            .ok()
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Self {
            api_key: env::var("ANTHROPIC_API_KEY").ok(),
            model: env::var("SYSGUD_MODEL").unwrap_or_else(|_| "claude-sonnet-5".to_string()),
            context_lines: env::var("SYSGUD_CONTEXT_LINES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(12),
            target_program,
            target_args,
            telegram_bot_token,
            telegram_chat_id,
            telegram_allowlist,
        }
    }
}

/// Comando de demostración: un script de Python que simula un OOM.
/// Se usa solo cuando no se especifica `SYSGUD_TARGET_ARGS`.
fn default_demo_args() -> Vec<String> {
    vec![
        "-c".to_string(),
        "import time, sys; print('System running...'); time.sleep(1); \
         print('CRITICAL ERROR: OutOfMemory in process_data()'); sys.exit(1)"
            .to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn from_env_parses_telegram_fields() {
        env::set_var("TELEGRAM_BOT_TOKEN", "bot-token-abc");
        env::set_var("TELEGRAM_CHAT_ID", "12345");
        env::set_var("TELEGRAM_ALLOWLIST", "111, 222, 333");

        let config = Config::from_env();

        assert_eq!(config.telegram_bot_token, Some("bot-token-abc".to_string()));
        assert_eq!(config.telegram_chat_id, Some("12345".to_string()));
        assert_eq!(config.telegram_allowlist, vec!["111", "222", "333"]);

        env::remove_var("TELEGRAM_BOT_TOKEN");
        env::remove_var("TELEGRAM_CHAT_ID");
        env::remove_var("TELEGRAM_ALLOWLIST");
    }

    #[test]
    fn from_env_telegram_fields_missing_when_unset() {
        env::remove_var("TELEGRAM_BOT_TOKEN");
        env::remove_var("TELEGRAM_CHAT_ID");
        env::remove_var("TELEGRAM_ALLOWLIST");

        let config = Config::from_env();

        assert_eq!(config.telegram_bot_token, None);
        assert_eq!(config.telegram_chat_id, None);
        assert!(config.telegram_allowlist.is_empty());
    }

    #[test]
    fn from_env_allowlist_empty_string_is_empty_vec() {
        env::set_var("TELEGRAM_ALLOWLIST", "");
        let config = Config::from_env();
        assert!(config.telegram_allowlist.is_empty());
        env::remove_var("TELEGRAM_ALLOWLIST");
    }
}
