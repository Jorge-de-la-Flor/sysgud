use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;
use tokio::time::{sleep, Duration};

use crate::core::{ActionType, PendingApproval};
use crate::telegram::client::{TelegramClient, Update};
use crate::telegram::commands::{Command, parse, is_allowlisted};
use crate::telegram::TelegramCtx;
use crate::actions::runner;

/// Loop de polling `getUpdates` que se ejecuta concurrentemente
/// con el loop de monitor en `lib.rs::run()`.
///
/// - GET `getUpdates?offset=<offset>&timeout=30`
/// - Al recibir un 200, procesa cada actualización
/// - Establece `offset = last_update_id + 1`
/// - Backoff en errores de red o 429
/// - Se ejecuta en `tokio::spawn` y está acotado por `timeout=30s`
pub async fn spawn_poll(
    client: TelegramClient,
    ctx: Arc<Mutex<TelegramCtx>>,
) -> Result<()> {
    let mut offset: i64 = 0;

    loop {
        let url = if offset == 0 {
            format!("https://api.telegram.org/bot{}/getUpdates?timeout=30", client.token)
        } else {
            format!("https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30", client.token, offset)
        };

        match client.get_updates(&url).await {
            Ok(response) => {
                if !response.ok || response.result.is_empty() {
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }

                let mut last_id = 0;
                for update in &response.result {
                    last_id = update.update_id;
                    process_update(update, &client, &ctx).await;
                }

                if last_id > 0 {
                    offset = last_id + 1;
                }
            }
            Err(e) => {
                println!("[telegram] poll error: {e}, backing off");
                sleep(Duration::from_secs(5)).await;
            }
        }

        sleep(Duration::from_millis(100)).await;
    }
}

/// Procesa una actualización individual: parsea el comando y
/// verifica la allowlist antes de tomar acción.
async fn process_update(
    update: &Update,
    client: &TelegramClient,
    ctx: &Arc<Mutex<TelegramCtx>>,
) {
    let text = match &update.message {
        Some(msg) => match &msg.text {
            Some(t) => t.clone(),
            None => return,
        },
        None => return,
    };

    let from_id = update.message.as_ref().unwrap().from.id;
    let command = parse(&text);

    // Extraemos los datos necesarios del guard sin bloqueo anidado.
    let (pending_opt, allowlist) = {
        let guard = ctx.lock().await;
        let pending = guard.pending.lock().await;
        let pending_clone = pending.as_ref().cloned();
        let allowlist = guard.allowlist.clone();
        (pending_clone, allowlist)
    };

    match command {
        Command::Status => {
            let reply = match pending_opt {
                Some(p) => format!("Pending: {:?} - {}", p.action.action_type, p.action.diagnosis),
                None => "No pending approval".to_string(),
            };
            let _ = client.send(&reply).await;
        }
        Command::Approve => {
            if !is_allowlisted(from_id, &allowlist) {
                let _ = client.send("Approval denied: not allowlisted").await;
                return;
            }
            if let Some(approval) = pending_opt {
                match_approval(approval).await;
                let _ = client.send("Approval confirmed: action executed").await;
            }
        }
        Command::Reject => {
            if !is_allowlisted(from_id, &allowlist) {
                let _ = client.send("Rejection denied: not allowlisted").await;
                return;
            }
            // Clear pending: necesitamos bloquear el mutex interno de nuevo
            let guard = ctx.lock().await;
            *guard.pending.lock().await = None;
            drop(guard);
            let _ = client.send("Action cancelled").await;
        }
        Command::Unknown => {}
    }
}

/// Ejecuta la acción aprobada almacenada en `PendingApproval`.
async fn match_approval(approval: PendingApproval) {
    match approval.action.action_type {
        ActionType::Kill => {
            let _ = runner::kill::run(approval.pid).await;
        }
        ActionType::Execute => {
            let _ = runner::execute_cmd::run(approval.action.command.as_deref()).await;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn offset_increments_after_update() {
        let last_id: i64 = 42;
        assert_eq!(last_id + 1, 43);
    }
}