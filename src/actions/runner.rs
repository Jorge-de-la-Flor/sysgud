//! Agrupa los submódulos de remediación: uno por cada [`ActionType`].
//!
//! Este archivo cumple el rol que antes tenía `runner/mod.rs`: convive
//! con la carpeta `runner/` y declara sus hijos.

pub mod execute_cmd;
pub mod kill;
mod notify;

use anyhow::Result;
use colored::*;

use crate::core::{ActionType, AgentAction, PendingApproval};
use crate::telegram::TelegramCtx;

/// Despacha la decisión del agente al manejador correspondiente en el SO.
///
/// `target_pid` es el PID del proceso supervisado, capturado por
/// `crate::monitor`; se usa únicamente para la acción `Kill`.
///
/// `ctx` es el contexto compartido de Telegram que contiene la
/// aprobación pendiente y la allowlist. Si `ctx.client` es `None`,
/// el comportamiento es igual que antes (sin gate de Telegram).
pub async fn execute(
    action: AgentAction,
    target_pid: Option<u32>,
    ctx: &TelegramCtx,
) -> Result<()> {
    println!("{}", "\n--- AGENT RESOLUTION ---".cyan().bold());
    println!("Diagnosis: {}", action.diagnosis.white());
    println!("Action Type: {:?}", action.action_type);

    match action.action_type {
        ActionType::Notify => { notify::run(&action.diagnosis).await }
        ActionType::None => {
            println!("{}", "[i] No automated action executed.".dimmed());
            Ok(())
        }
        ActionType::Kill | ActionType::Execute => {
            // Gate: Kill/Execute requieren aprobación previa
            // si hay un pending sin confirmar.
            {
                let pending_guard = ctx.pending.lock().await;
                if pending_guard.is_some() {
                    drop(pending_guard);
                    return Ok(());
                }
            }

            {
                let mut guard = ctx.pending.lock().await;
                *guard = Some(PendingApproval {
                    action: action.clone(),
                    pid: target_pid,
                });
            }

            if let Some(client) = &ctx.client {
                let _ = client
                    .send(&format!("Action {:?} pending approval. Confirm with /approve", action.action_type))
                    .await;
            }

            println!(
                "[gate] {:?} action stored pending approval. Awaiting /approve.",
                action.action_type
            );
            Ok(())
        }
    }
}