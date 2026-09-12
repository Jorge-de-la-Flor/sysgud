use crate::commands::{parse, Command};
use anyhow::{anyhow, ensure, Result};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{collections::HashSet, time::Duration};
use sysgud_core::Incident;

#[derive(Clone)]
pub struct Bot {
    http: reqwest::Client,
    local: reqwest::Client,
    token: String,
    api_token: String,
    api_url: String,
    telegram_url: String,
    allowed: HashSet<i64>,
}
#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
}
#[derive(Deserialize)]
struct User {
    id: i64,
    #[serde(default)]
    is_bot: bool,
}
#[derive(Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    kind: String,
}
#[derive(Deserialize)]
struct Message {
    from: Option<User>,
    chat: Chat,
    text: Option<String>,
}
#[derive(Deserialize)]
struct Update {
    update_id: i64,
    message: Option<Message>,
}

impl Bot {
    pub fn new(token: String, api_token: String, port: u16, users: &str) -> Result<Self> {
        let (prefix, secret) = token
            .split_once(':')
            .ok_or_else(|| anyhow!("formato de TELEGRAM_BOT_TOKEN inválido"))?;
        ensure!(
            !prefix.is_empty()
                && prefix.bytes().all(|c| c.is_ascii_digit())
                && secret.len() >= 20
                && secret
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
            "formato de TELEGRAM_BOT_TOKEN inválido"
        );
        let allowed: HashSet<i64> = users
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().parse())
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            !allowed.is_empty() && allowed.iter().all(|id| *id > 0),
            "Telegram requiere usuarios permitidos"
        );
        let make = || {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(35))
        };
        Ok(Self {
            http: make().build()?,
            local: make().no_proxy().build()?,
            token,
            api_token,
            api_url: format!("http://127.0.0.1:{port}/api/v1"),
            telegram_url: "https://api.telegram.org".into(),
            allowed,
        })
    }
    fn authorized_message(&self, message: &Message) -> Option<i64> {
        let user = message.from.as_ref()?;
        (message.chat.kind == "private"
            && message.chat.id == user.id
            && !user.is_bot
            && self.allowed.contains(&user.id))
        .then_some(user.id)
    }
    async fn telegram<T: DeserializeOwned>(&self, method: &str, body: Value) -> Result<T> {
        // Never include reqwest errors here: their URL contains the bot token.
        let response = self
            .http
            .post(format!("{}/bot{}/{method}", self.telegram_url, self.token))
            .json(&body)
            .send()
            .await
            .map_err(|_| anyhow!("Telegram: error de conexión"))?;
        let envelope: Envelope<T> = bounded_json(response)
            .await
            .map_err(|_| anyhow!("Telegram: respuesta inválida o servicio no disponible"))?;
        ensure!(envelope.ok, "Telegram rechazó la solicitud");
        envelope
            .result
            .ok_or_else(|| anyhow!("Telegram no confirmó el resultado"))
    }
    async fn api<T: DeserializeOwned>(&self, path: &str, decision: Option<i64>) -> Result<T> {
        let url = format!("{}{path}", self.api_url);
        let request = if let Some(actor) = decision {
            self.local.post(url).json(&json!({"actor_id":actor}))
        } else {
            self.local.get(url)
        };
        let response = request
            .bearer_auth(&self.api_token)
            .send()
            .await
            .map_err(|_| anyhow!("API local no disponible"))?;
        bounded_json(response).await.map_err(|_| anyhow!("La API no confirmó la operación. Consulta /status; no se ejecuta un segundo intento automático."))
    }
    async fn send(&self, chat: i64, text: &str) -> Result<()> {
        let _: Value = self
            .telegram(
                "sendMessage",
                json!({"chat_id":chat,"text":text,"link_preview_options":{"is_disabled":true}}),
            )
            .await?;
        Ok(())
    }
    async fn handle(&self, update: &Update) -> Result<()> {
        let Some(message) = update.message.as_ref() else {
            return Ok(());
        };
        let Some(actor) = self.authorized_message(message) else {
            return Ok(());
        };
        let command = parse(message.text.as_deref().unwrap_or_default());
        let text = match command {
            Command::Status => {
                let items: Vec<Incident> = self.api("/incidents?status=pending_approval&limit=10", None).await?;
                if items.is_empty() { "No hay incidentes pendientes.".into() }
                else { items.iter().map(|i| format!("{} — {:?}\n/incident {}", i.id, i.proposed_action.action_type, i.id)).collect::<Vec<_>>().join("\n\n") }
            }
            Command::Detail(id) => {
                let item: Incident = self.api(&format!("/incidents/{id}"), None).await?;
                detail(&item)
            }
            Command::Approve(id) | Command::Reject(id) => {
                let route = if matches!(command, Command::Approve(_)) { "approve" } else { "reject" };
                let item: Incident = self.api(&format!("/incidents/{id}/{route}"), Some(actor)).await?;
                format!("Incidente {}: {:?}.\nConsulta /incident {} para el resultado.\n{}", item.id, item.status, item.id, item.error.as_deref().unwrap_or(""))
            }
            Command::Help => "sysgud\n/status — pendientes\n/incident ID — revisar propuesta y comando exacto\n/approve ID — aprobar\n/reject ID — rechazar\nLas acciones se identifican por incidente; ninguna aprobación global.".into(),
            Command::Unknown => return Ok(()),
        };
        self.send(actor, &text).await
    }
    pub async fn run(self) {
        let mut offset = 0i64;
        loop {
            let response: Result<Vec<Update>> = self
                .telegram(
                    "getUpdates",
                    json!({"offset":offset,"timeout":25,"limit":20,"allowed_updates":["message"]}),
                )
                .await;
            match response {
                Ok(updates) => {
                    for update in updates {
                        if update.update_id < offset {
                            continue;
                        }
                        if let Err(error) = self.handle(&update).await {
                            eprintln!("{error}");
                            if let Some(actor) = update
                                .message
                                .as_ref()
                                .and_then(|m| self.authorized_message(m))
                            {
                                let _ = self.send(actor, &error.to_string()).await;
                            }
                        }
                        offset = update.update_id.saturating_add(1).max(offset);
                    }
                }
                Err(error) => {
                    eprintln!("{error}. Revise token, conectividad y que el bot no tenga otro polling o webhook activo.");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
    pub async fn notifications(
        self,
        chat: i64,
        mut receiver: tokio::sync::broadcast::Receiver<Incident>,
    ) {
        if !self.allowed.contains(&chat) {
            eprintln!("TELEGRAM_CHAT_ID debe ser el chat privado de un usuario permitido; notificaciones desactivadas");
            return;
        }
        loop {
            match receiver.recv().await {
                Ok(incident) => {
                    if let Err(error) = self.send(chat, &detail(&incident)).await {
                        eprintln!("Notificación no confirmada: {error}. El incidente sigue disponible en la API.");
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("Telegram: {n} avisos omitidos; consulte /status")
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

fn detail(item: &Incident) -> String {
    let mut text = format!(
        "Incidente {}\nEstado: {:?}\nAcción: {:?}\n{}",
        item.id,
        item.status,
        item.proposed_action.action_type,
        item.diagnosis.chars().take(1000).collect::<String>()
    );
    if let Some(spec) = &item.execution {
        let invocation = serde_json::to_string(spec).unwrap_or_default();
        // Never invite approval of a truncated command.
        if invocation.chars().count() > 1800 {
            return format!("{text}\nEl comando es demasiado largo para revisarlo aquí. Revísalo completo mediante GET /api/v1/incidents/{} antes de aprobar por API.", item.id);
        }
        text.push_str(&format!("\nInvocación literal: {invocation}"));
    }
    text.push_str(&format!("\n/approve {}\n/reject {}", item.id, item.id));
    text
}

async fn bounded_json<T: DeserializeOwned>(mut response: reqwest::Response) -> Result<T> {
    ensure!(response.status().is_success(), "HTTP no exitoso");
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("lectura HTTP fallida"))?
    {
        ensure!(
            bytes.len() + chunk.len() <= 1024 * 1024,
            "respuesta HTTP demasiado grande"
        );
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| anyhow!("JSON inválido"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn telegram_approval_uses_the_authenticated_api_and_reports_reserved_state() {
        use axum::{routing::post, Json, Router};
        use sysgud_runtime::{agent::AgentClient, AppState};
        let state = AppState::new(
            AgentClient::new(None, "test".into()),
            "x".repeat(32),
            "123",
            3,
        )
        .unwrap();
        let item = state
            .ingest("test".into(), "ERROR fixture".into(), None)
            .await
            .unwrap()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = sysgud_api::router(state.clone());
        let api = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (send, mut receive) = tokio::sync::mpsc::channel(4);
        let app = Router::new().route(
            "/{*path}",
            post(move |Json(body): Json<Value>| {
                let send = send.clone();
                async move {
                    send.send(body).await.unwrap();
                    Json(json!({"ok":true,"result":{}}))
                }
            }),
        );
        let telegram = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut bot = Bot::new(
            "123456:abcdefghijklmnopqrstuvwxyz0123456789".into(),
            "x".repeat(32),
            port,
            "123",
        )
        .unwrap();
        bot.telegram_url = format!("http://{address}");
        for actor in [999, 123] {
            let update: Update = serde_json::from_value(json!({"update_id":actor,"message":{"from":{"id":actor},"chat":{"id":actor,"type":"private"},"text":format!("/approve {}",item.id)}})).unwrap();
            bot.handle(&update).await.unwrap();
            if actor == 999 {
                assert!(receive.try_recv().is_err());
                assert_eq!(
                    state.get(item.id).await.unwrap().status,
                    sysgud_core::IncidentStatus::PendingApproval
                );
            }
        }
        let reply = receive.recv().await.unwrap();
        assert_eq!(reply["chat_id"], 123);
        assert!(reply["text"].as_str().unwrap().contains("Executing"));
        state.shutdown().await;
        assert_eq!(state.get(item.id).await.unwrap().decided_by, Some(123));
        api.abort();
        telegram.abort();
    }
    fn bot() -> Bot {
        Bot::new(
            "123456:abcdefghijklmnopqrstuvwxyz0123456789".into(),
            "x".repeat(32),
            3000,
            "123",
        )
        .unwrap()
    }
    #[test]
    fn denies_empty_allowlist() {
        assert!(Bot::new(
            "123456:abcdefghijklmnopqrstuvwxyz0123456789".into(),
            "x".repeat(32),
            3000,
            ""
        )
        .is_err());
    }
    #[test]
    fn real_sender_and_private_chat_are_both_required() {
        for (user, chat, kind, allowed) in [
            (123, 123, "private", true),
            (999, 123, "private", false),
            (123, -123, "group", false),
            (123, 999, "private", false),
        ] {
            let message: Message = serde_json::from_value(
                json!({"from":{"id":user},"chat":{"id":chat,"type":kind},"text":"/approve 999"}),
            )
            .unwrap();
            assert_eq!(bot().authorized_message(&message).is_some(), allowed);
        }
    }
}
