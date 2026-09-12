//! Orquestación de alto nivel de sysgud.
//!
//! Cablea los cuatro módulos principales de la arquitectura:
//!
//! - [`monitor`]: ingesta del proceso objetivo + buffer circular.
//! - [`agent`]: construcción del payload y llamada al LLM.
//! - [`actions`]: ejecución de la remediación decidida por el agente.
//! - [`telegram`]: notificaciones salientes y comandos entrantes.
//!
//! `core` contiene los tipos, config y errores compartidos entre los cuatro.

pub mod actions;
pub mod agent;
pub mod core;
pub mod monitor;
pub mod telegram;

use colored::*;

use actions::execute as run_action;
use agent::AgentClient;
use core::{AgentRequest, Config};
use monitor::{is_trigger, spawn, RingBuffer};
use telegram::{is_enabled, TelegramCtx};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::task::JoinHandle;

/// Punto de entrada de la aplicación. Se ejecuta hasta que el
/// proceso objetivo termina o se recibe una señal de cierre.
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

    let telegram_ctx: Arc<Mutex<TelegramCtx>> = Arc::new(Mutex::new(TelegramCtx::new(&config)));
    let poll_ctx = telegram_ctx.clone();

    // Lanza el polling de Telegram concurrentemente con el monitor
    // solo cuando el módulo está habilitado.
    let poll_handle: Option<JoinHandle<Result<(), anyhow::Error>>> = if is_enabled(&config) {
        let client = {
            let ctx_guard = poll_ctx.lock().await;
            ctx_guard.client.clone()
        };
        client.map(|c| Some(tokio::spawn(telegram::updates::spawn_poll(c, poll_ctx)))).unwrap_or(None)
    } else {
        None
    };
    // poll_ctx is consumed by spawn_poll or dropped here

    // Handler para Ctrl+C: aborta el poll y termina limpiamente.
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let shutdown_flag_clone = shutdown_flag.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown_flag_clone.store(true, Ordering::SeqCst);
    });

    loop {
        if shutdown_flag.load(Ordering::SeqCst) {
            println!("{}", "\n[!] Shutdown signal received.".yellow());
            if let Some(h) = &poll_handle {
                h.abort();
            }
            break;
        }

        match handle.lines.recv().await {
            Some(line) => {
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
                    let ctx = telegram_ctx.lock().await;
                    run_action(action, target_pid, &ctx).await?;
                    // Se elimina el `break`: el loop continúa hasta
                    // que el proceso hijo termina o se recibe Ctrl+C.
                    drop(ctx);
                }
            }
            None => break,
        }
    }

    if let Some(h) = poll_handle {
        let _ = h.await;
    }
    let _ = handle.child.wait().await;
    Ok(())
}