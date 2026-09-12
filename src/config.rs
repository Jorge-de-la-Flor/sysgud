use anyhow::{anyhow, ensure, Result};
use std::env;

/// Lee el entorno existente. No carga ni modifica archivos .env.
#[derive(Clone)]
pub struct Config {
    pub api_key: Option<String>,
    pub model: String,
    pub context_lines: usize,
    pub target_program: String,
    pub target_args: Vec<String>,
    pub token: String,
    pub allowed_users: String,
    pub api_port: u16,
    pub api_host: std::net::Ipv4Addr,
    pub monitor_enabled: bool,
    pub telegram_enabled: bool,
    pub telegram_token: Option<String>,
    pub telegram_chat_id: Option<i64>,
    pub options: sysgud_runtime::ServiceOptions,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let target_program = env::var("SYSGUD_TARGET_CMD").unwrap_or_else(|_| {
            if cfg!(windows) {
                "python".into()
            } else {
                "python3".into()
            }
        });
        ensure!(
            !target_program.trim().is_empty(),
            "SYSGUD_TARGET_CMD no puede estar vacío"
        );
        let raw = env::var("SYSGUD_TARGET_ARGS").unwrap_or_default();
        let target_args = if raw.trim().is_empty() && env::var_os("SYSGUD_TARGET_CMD").is_none() {
            default_demo_args()
        } else if raw.trim().is_empty() {
            Vec::new()
        } else {
            parse_args(&raw)?
        };
        let context_lines = match env::var("SYSGUD_CONTEXT_LINES") {
            Ok(raw) => raw
                .parse::<usize>()
                .map_err(|_| anyhow!("SYSGUD_CONTEXT_LINES debe ser un entero"))?,
            Err(env::VarError::NotPresent) => 12,
            Err(error) => return Err(error.into()),
        };
        ensure!(
            (1..=100).contains(&context_lines),
            "SYSGUD_CONTEXT_LINES debe estar entre 1 y 100"
        );
        Ok(Self {
            api_key: env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            model: env::var("SYSGUD_MODEL").unwrap_or_else(|_| "claude-sonnet-5".into()),
            context_lines,
            target_program,
            target_args,
            token: env::var("SYSGUD_BOT_API_TOKEN").unwrap_or_default(),
            allowed_users: allowed_users()?,
            api_port: number("SYSGUD_API_PORT", 3000, 1, 65535)? as u16,
            api_host: parse_api_host(
                &env::var("SYSGUD_API_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            )?,
            monitor_enabled: boolean("SYSGUD_MONITOR_ENABLED", true)?,
            telegram_enabled: boolean("SYSGUD_TELEGRAM_ENABLED", false)?,
            telegram_token: env::var("TELEGRAM_BOT_TOKEN")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            telegram_chat_id: env::var("TELEGRAM_CHAT_ID")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.parse())
                .transpose()
                .map_err(|_| anyhow!("TELEGRAM_CHAT_ID debe ser numérico"))?,
            options: sysgud_runtime::ServiceOptions {
                database: Some(
                    env::var("SYSGUD_DATABASE")
                        .unwrap_or_else(|_| ".data/sysgud.sqlite".into())
                        .into(),
                ),
                max_incidents: number("SYSGUD_MAX_INCIDENTS", 1000, 1, 10000)?,
                approval_ttl: std::time::Duration::from_secs(number(
                    "SYSGUD_APPROVAL_TTL_SECONDS",
                    900,
                    1,
                    86400,
                )? as u64),
                commands: serde_json::from_str(
                    &env::var("SYSGUD_COMMANDS_JSON").unwrap_or_else(|_| "{}".into()),
                )
                .map_err(|_| anyhow!("SYSGUD_COMMANDS_JSON inválido"))?,
            },
        })
    }
}

fn parse_api_host(value: &str) -> Result<std::net::Ipv4Addr> {
    match value {
        "127.0.0.1" => Ok(std::net::Ipv4Addr::LOCALHOST),
        "0.0.0.0" => Ok(std::net::Ipv4Addr::UNSPECIFIED),
        _ => Err(anyhow!("SYSGUD_API_HOST debe ser 127.0.0.1 o 0.0.0.0")),
    }
}

fn allowed_users() -> Result<String> {
    let current = env::var("SYSGUD_ALLOWED_TELEGRAM_USER_IDS").unwrap_or_default();
    let previous = env::var("TELEGRAM_ALLOWLIST").unwrap_or_default();
    let normalize = |s: &str| -> Result<std::collections::BTreeSet<i64>> {
        if s.trim().is_empty() {
            return Ok(Default::default());
        }
        s.split(',')
            .map(|v| {
                v.trim()
                    .parse()
                    .map_err(|_| anyhow!("lista de usuarios inválida"))
            })
            .collect()
    };
    if !current.trim().is_empty() && !previous.trim().is_empty() {
        ensure!(
            normalize(&current)? == normalize(&previous)?,
            "TELEGRAM_ALLOWLIST y SYSGUD_ALLOWED_TELEGRAM_USER_IDS deben coincidir"
        );
    }
    Ok(if current.trim().is_empty() {
        previous
    } else {
        current
    })
}

fn number(name: &str, default: usize, min: usize, max: usize) -> Result<usize> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| anyhow!("{name} debe ser entero"))?,
        Err(env::VarError::NotPresent) => default,
        Err(_) => return Err(anyhow!("{name} inválido")),
    };
    ensure!(
        (min..=max).contains(&value),
        "{name} fuera de rango {min}–{max}"
    );
    Ok(value)
}
pub fn boolean(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(anyhow!("{name} debe ser true o false")),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(_) => Err(anyhow!("{name} inválido")),
    }
}

/// JSON permite todos los argumentos literalmente, incluido [] para ninguno.
/// La sintaxis tradicional admite espacios entre argumentos y comillas agrupadoras;
/// no interpreta escapes ni expande variables del shell.
fn parse_args(raw: &str) -> Result<Vec<String>> {
    if raw.trim_start().starts_with('[') {
        return Ok(serde_json::from_str(raw)?);
    }
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for ch in raw.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                started = true;
            }
            None if ch.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(ch);
                started = true;
            }
        }
    }
    ensure!(
        quote.is_none(),
        "comillas sin cerrar en SYSGUD_TARGET_ARGS; use un array JSON para argumentos complejos"
    );
    if started {
        args.push(current);
    }
    Ok(args)
}

fn default_demo_args() -> Vec<String> {
    vec!["-u".into(), "-c".into(), "import time, sys; print('System running...'); time.sleep(1); print('CRITICAL ERROR: OutOfMemory in process_data()'); sys.exit(1)".into()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_binding_requires_an_explicit_supported_address() {
        assert_eq!(
            parse_api_host("127.0.0.1").unwrap(),
            std::net::Ipv4Addr::LOCALHOST
        );
        assert_eq!(
            parse_api_host("0.0.0.0").unwrap(),
            std::net::Ipv4Addr::UNSPECIFIED
        );
        for invalid in ["", "localhost", "::", "192.168.1.1", "0.0.0.0:3000"] {
            assert!(parse_api_host(invalid).is_err());
        }
    }

    #[test]
    fn parses_windows_paths_quotes_and_empty_arguments() {
        assert_eq!(
            parse_args(r#"--file "C:\My Folder\file.txt" ''"#).unwrap(),
            vec!["--file", r"C:\My Folder\file.txt", ""]
        );
        assert!(parse_args("[]").unwrap().is_empty());
        assert_eq!(
            parse_args(r#"["-c", "print(\"hello world\")"]"#).unwrap(),
            vec!["-c", "print(\"hello world\")"]
        );
        assert!(parse_args("'unclosed").is_err());
        assert!(parse_args("[invalid]").is_err());
    }
}
