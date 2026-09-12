use anyhow::Result;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc::{self, Receiver, Sender};

pub const MAX_LINE_BYTES: usize = 8192;
const QUEUE_SIZE: usize = 256;

pub struct ReaderHandle {
    pub child: Child,
    pub lines: Receiver<String>,
    pub dropped_lines: Arc<AtomicU64>,
}

/// Lectura por bytes: acota memoria incluso si no llega un salto de línea.
async fn read_stream<R: AsyncBufRead + Unpin>(
    mut reader: R,
    tx: Sender<String>,
    dropped: Arc<AtomicU64>,
) {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let available = match reader.fill_buf().await {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("Error leyendo salida supervisada: {error}");
                break;
            }
        };
        if available.is_empty() {
            if !line.is_empty() || oversized {
                emit_line(&tx, &dropped, &line, oversized);
            }
            break;
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(available.len(), |offset| offset + 1);
        if !oversized {
            if line.len() + count > MAX_LINE_BYTES {
                oversized = true;
                line.clear();
            } else {
                line.extend_from_slice(&available[..count]);
            }
        }
        reader.consume(count);
        if end.is_some() {
            if tx.is_closed() {
                break;
            }
            emit_line(&tx, &dropped, &line, oversized);
            line.clear();
            oversized = false;
        }
    }
}

fn emit_line(tx: &Sender<String>, dropped: &AtomicU64, bytes: &[u8], oversized: bool) {
    if oversized {
        dropped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let line = String::from_utf8_lossy(bytes)
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    if tx.try_send(line).is_err() {
        dropped.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn spawn(program: &str, args: &[&str]) -> Result<ReaderHandle> {
    let mut command = Command::new(program);
    command
        .args(args)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    crate::security::protect_environment(&mut command);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (tx, lines) = mpsc::channel(QUEUE_SIZE);
    let dropped_lines = Arc::new(AtomicU64::new(0));
    tokio::spawn(read_stream(
        BufReader::new(stdout),
        tx.clone(),
        dropped_lines.clone(),
    ));
    tokio::spawn(read_stream(
        BufReader::new(stderr),
        tx,
        dropped_lines.clone(),
    ));
    Ok(ReaderHandle {
        child,
        lines,
        dropped_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn handles_invalid_utf8_final_lines_and_discards_oversized_lines() {
        let mut bytes = vec![b'x'; MAX_LINE_BYTES + 1];
        bytes.extend_from_slice(b"\nERROR: normal\ninvalid:\xff\nfinal");
        let (tx, mut rx) = mpsc::channel(8);
        let dropped = Arc::new(AtomicU64::new(0));
        read_stream(BufReader::new(bytes.as_slice()), tx, dropped.clone()).await;
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert_eq!(rx.recv().await.as_deref(), Some("ERROR: normal"));
        assert!(rx.recv().await.unwrap().contains('\u{fffd}'));
        assert_eq!(rx.recv().await.as_deref(), Some("final"));
        assert!(rx.recv().await.is_none());
    }
    #[tokio::test]
    async fn full_queue_does_not_block_the_child_reader() {
        let (tx, _rx) = mpsc::channel(1);
        let dropped = Arc::new(AtomicU64::new(0));
        read_stream(
            BufReader::new(&b"one\ntwo\nthree\n"[..]),
            tx,
            dropped.clone(),
        )
        .await;
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }
}
