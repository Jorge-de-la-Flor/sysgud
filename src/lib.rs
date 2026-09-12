//! Orquestación de alto nivel de sysgud.
//!
//! Cablea los tres módulos principales de la arquitectura:
//!
//! - [`monitor`]: ingesta del proceso objetivo + buffer circular.
//! - [`agent`]: construcción del payload y llamada al LLM.
//! - [`actions`]: ejecución de la remediación decidida por el agente.
//!
//! `core` contiene los tipos, config y errores compartidos entre los tres.

pub mod actions;
pub mod agent;
pub mod core;
pub mod monitor;

use colored::*;

use actions::execute as run_action;
use agent::AgentClient;
use core::{AgentRequest, Config};
use monitor::{is_trigger, spawn, RingBuffer};

/// Punto de entrada de la aplicación. Se ejecuta hasta detectar el
/// primer evento de falla, delegar el diagnóstico al agente y aplicar
/// la remediación resultante.
pub async fn run() -> anyhow::Result<()> {
    println!(
        "{}",
        "=== SystemGuard Runtime Agent Active ===".green().bold()
    );

    let config = Config::from_env();
    let args: Vec<&str> = config.target_args.iter().map(String::as_str).collect();

    let mut handle = spawn(&config.target_program, &args)?;
    let target_pid = handle.child.id();

    let mut buffer = RingBuffer::new(config.context_lines);
    let agent = AgentClient::new(config.api_key.clone(), config.model.clone());

    while let Some(line) = handle.lines.recv().await {
        println!("[SYS LOG] {}", line);
        buffer.push(line.clone());

        if is_trigger(&line) {
            println!(
                "{}",
                "\n[!] Target event detected. Invoking System Agent..."
                    .yellow()
                    .bold()
            );

            let request = AgentRequest {
                system_context: "Rust async supervised process".to_string(),
                log_extract: buffer.snapshot(),
            };

            let action = agent.analyze(request).await?;
            run_action(action, target_pid).await?;
            break;
        }
    }

    let _ = handle.child.wait().await;
    Ok(())
}
