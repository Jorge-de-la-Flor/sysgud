//! Composition root: configuration and lifecycle; crates enforce capability boundaries.
mod config;
use runtime::{agent::AgentClient, AppState, Target};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
pub use sysgud_api as api;
pub use sysgud_core as core;
pub use sysgud_runtime as runtime;
use tokio::sync::{watch, Mutex};

pub async fn run() -> anyhow::Result<()> {
    if config::boolean("SYSGUD_LOAD_DOTENV", true)? {
        match dotenvy::dotenv() {
            Ok(_) => {}
            Err(dotenvy::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => anyhow::bail!("no se pudo cargar .env; revise su formato"),
        }
    }
    let config = config::Config::from_env()?;
    let state = AppState::with_options(
        AgentClient::new(config.api_key.clone(), config.model.clone()),
        config.token.clone(),
        &config.allowed_users,
        config.context_lines,
        config.options.clone(),
    )?;
    let telegram = if config.telegram_enabled {
        if let Some(chat) = config.telegram_chat_id {
            anyhow::ensure!(
                state.actor_allowed(chat),
                "TELEGRAM_CHAT_ID debe ser el chat privado de un usuario permitido"
            );
        }
        Some(sysgud_telegram::Bot::new(
            config
                .telegram_token
                .clone()
                .ok_or_else(|| anyhow::anyhow!("falta TELEGRAM_BOT_TOKEN"))?,
            config.token.clone(),
            config.api_port,
            &config.allowed_users,
        )?)
    } else {
        None
    };
    if std::env::args().any(|arg| arg == "--check") {
        println!(
            "Configuración válida. Monitor: {}; Telegram: {}; LLM: {}.",
            config.monitor_enabled,
            config.telegram_enabled,
            config.api_key.is_some()
        );
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind((config.api_host, config.api_port)).await?;
    println!("sysgud API: http://{}", listener.local_addr()?);
    let (stop, receiver) = watch::channel(false);
    let notifications = telegram
        .clone()
        .zip(config.telegram_chat_id)
        .map(|(bot, chat)| {
            let events = state.subscribe();
            tokio::spawn(async move { bot.notifications(chat, events).await })
        });
    let monitor = if config.monitor_enabled {
        let monitor_state = state.clone();
        Some(tokio::spawn(async move {
            if let Err(error) = run_monitor(config, monitor_state.clone(), receiver).await {
                eprintln!(
                    "Monitor detenido: {}",
                    monitor_state.redact(&error.to_string())
                );
            }
        }))
    } else {
        None
    };
    let telegram = telegram.map(|bot| tokio::spawn(async move { bot.run().await }));
    let shutdown_state = state.clone();
    let result = axum::serve(listener, api::router(state.clone()))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            let _ = stop.send(true);
            shutdown_state.shutdown().await;
        })
        .await;
    if let Some(task) = telegram {
        task.abort();
        let _ = task.await;
    }
    if let Some(task) = notifications {
        task.abort();
        let _ = task.await;
    }
    if let Some(mut task) = monitor {
        if tokio::time::timeout(Duration::from_secs(5), &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
        }
    }
    state.shutdown().await;
    result?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn run_monitor(
    config: config::Config,
    state: AppState,
    mut stop: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let args: Vec<&str> = config.target_args.iter().map(String::as_str).collect();
    let handle = runtime::monitor::spawn(&config.target_program, &args)?;
    let target: Target = Arc::new(Mutex::new(handle.child));
    let mut lines = handle.lines;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut exited_at = None;
    let mut streams_closed = false;
    let mut last_analysis = None;
    loop {
        tokio::select! {
            _ = stop.changed() => {
                let mut child = target.lock().await;
                if child.try_wait()?.is_none() { child.kill().await?; }
                break;
            }
            _ = ticker.tick() => {
                let dropped = handle.dropped_lines.swap(0, Ordering::Relaxed);
                if dropped > 0 { eprintln!("Monitor: {dropped} líneas descartadas por tamaño o saturación"); }
                if target.lock().await.try_wait()?.is_some() {
                    let when = exited_at.get_or_insert_with(tokio::time::Instant::now);
                    if streams_closed || when.elapsed() >= Duration::from_secs(2) { break; }
                }
            }
            line = lines.recv(), if !streams_closed => {
                let Some(line) = line else { streams_closed = true; continue; };
                // Bound paid analyses while continuing to drain the child pipes.
                let trigger = runtime::monitor::is_trigger(&line);
                if trigger && last_analysis.is_some_and(|last: tokio::time::Instant| last.elapsed() < Duration::from_secs(10)) { continue; }
                if trigger { last_analysis = Some(tokio::time::Instant::now()); }
                let ingest = state.ingest(config.target_program.clone(), line, Some(target.clone()));
                tokio::select! {
                    result = ingest => match result {
                        Ok(Some(incident)) => println!("Incidente {}: {:?}; pendiente de aprobación", incident.id, incident.proposed_action.action_type),
                        Ok(None) => {},
                        Err(error) => eprintln!("Monitor: {error}"),
                    },
                    _ = stop.changed() => {
                        let mut child = target.lock().await;
                        if child.try_wait()?.is_none() { child.kill().await?; }
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
