use anyhow::Result;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{self, UnboundedReceiver};

/// Manija sobre un proceso objetivo en ejecución: da acceso al `Child`
/// (para leer el PID o esperar su finalización) y a un canal unificado
/// de líneas provenientes tanto de stdout como de stderr.
pub struct ReaderHandle {
    pub child: Child,
    pub lines: UnboundedReceiver<String>,
}

/// Lanza `program` con `args`, capturando stdout y stderr de forma
/// asíncrona y no bloqueante. Ambos streams se fusionan en un único
/// canal para que el llamador no tenga que hacer `select!` manualmente.
pub fn spawn(program: &str, args: &[&str]) -> Result<ReaderHandle> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout fue configurado como piped");
    let stderr = child.stderr.take().expect("stderr fue configurado como piped");

    let (tx, rx) = mpsc::unbounded_channel();

    let tx_stdout = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx_stdout.send(line).is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    Ok(ReaderHandle { child, lines: rx })
}
