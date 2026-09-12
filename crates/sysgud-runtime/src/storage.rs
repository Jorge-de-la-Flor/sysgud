use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    sync::{Arc, Mutex},
};
use sysgud_core::{Decision, Incident};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Record {
    pub incident: Incident,
    pub decision: Option<Decision>,
}

#[derive(Clone)]
pub(crate) struct Storage {
    connection: Arc<Mutex<Connection>>,
    // One daemon owns a database: avoids divergent caches and duplicate effects.
    _lock: Option<Arc<File>>,
}

impl Storage {
    pub fn open(path: Option<&Path>, capacity: usize) -> Result<(Self, Vec<Record>)> {
        let (connection, lock) = if let Some(path) = path {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
                }
            }
            let lock = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path.with_extension("lock"))?;
            fs2::FileExt::try_lock_exclusive(&lock)
                .context("otra instancia ya usa esta base de datos")?;
            // Create with restricted permissions before SQLite opens it.
            let mut options = OpenOptions::new();
            options.create(true).truncate(false).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)?;
            (Connection::open(path)?, Some(Arc::new(lock)))
        } else {
            (Connection::open_in_memory()?, None)
        };
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS incidents(id TEXT PRIMARY KEY, data TEXT NOT NULL);",
        )?;
        let records = {
            let mut statement = connection.prepare("SELECT data FROM incidents LIMIT ?1")?;
            let rows = statement.query_map([capacity as i64 + 1], |row| row.get::<_, String>(0))?;
            let mut records = Vec::new();
            for row in rows {
                let data = row?;
                anyhow::ensure!(
                    data.len() <= 1024 * 1024,
                    "registro almacenado demasiado grande"
                );
                records.push(serde_json::from_str(&data)?);
            }
            anyhow::ensure!(
                records.len() <= capacity,
                "la base supera SYSGUD_MAX_INCIDENTS"
            );
            records
        };
        Ok((
            Self {
                connection: Arc::new(Mutex::new(connection)),
                _lock: lock,
            },
            records,
        ))
    }

    pub async fn save(&self, record: Record, evict: Option<uuid::Uuid>) -> Result<()> {
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut connection = connection
                .lock()
                .map_err(|_| anyhow::anyhow!("storage lock failed"))?;
            let tx = connection.transaction()?;
            if let Some(id) = evict {
                tx.execute("DELETE FROM incidents WHERE id=?1", [id.to_string()])?;
            }
            tx.execute(
                "INSERT INTO incidents(id,data) VALUES(?1,?2)
                ON CONFLICT(id) DO UPDATE SET data=excluded.data",
                params![
                    record.incident.id.to_string(),
                    serde_json::to_string(&record)?
                ],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await??;
        Ok(())
    }
}
