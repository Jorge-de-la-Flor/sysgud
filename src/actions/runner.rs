//! Agrupa los submódulos de remediación: uno por cada [`ActionType`].
//!
//! Este archivo cumple el rol que antes tenía `runner/mod.rs`: convive
//! con la carpeta `runner/` y declara sus hijos.

mod execute_cmd;
mod kill;
mod notify;

use anyhow::Result;
use colored::*;

use crate::core::{ActionType, AgentAction};

/// Despacha la decisión del agente al manejador correspondiente en el SO.
///
/// `target_pid` es el PID del proceso supervisado, capturado por
/// `crate::monitor`; se usa únicamente para la acción `Kill`.
pub async fn execute(action: AgentAction, target_pid: Option<u32>) -> Result<()> {
    println!("{}", "\n--- AGENT RESOLUTION ---".cyan().bold());
    println!("Diagnosis: {}", action.diagnosis.white());
    println!("Action Type: {:?}", action.action_type);

    match action.action_type {
        ActionType::Kill => kill::run(target_pid).await,
        ActionType::Execute => execute_cmd::run(action.command.as_deref()).await,
        ActionType::Notify => notify::run(&action.diagnosis),
        ActionType::None => {
            println!("{}", "[i] No automated action executed.".dimmed());
            Ok(())
        }
    }
}
