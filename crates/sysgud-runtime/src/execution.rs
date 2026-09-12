use crate::Target;
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use sysgud_core::{ActionType, AgentAction};

pub use sysgud_core::types::CommandSpec;

pub(crate) async fn execute(
    action: AgentAction,
    target: Option<Target>,
    commands: &BTreeMap<String, CommandSpec>,
) -> anyhow::Result<()> {
    match action.action_type {
        ActionType::Kill => {
            let target = target.ok_or_else(|| anyhow::anyhow!("no hay hijo supervisado"))?;
            let mut child = target.lock().await;
            anyhow::ensure!(child.try_wait()?.is_none(), "el proceso ya terminó");
            child.kill().await?;
        }
        ActionType::Execute => {
            let spec = action
                .command
                .as_ref()
                .and_then(|id| commands.get(id))
                .ok_or_else(|| anyhow::anyhow!("comando fuera de la política"))?;
            let mut command = tokio::process::Command::new(&spec.program);
            command
                .args(&spec.args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            crate::security::protect_environment(&mut command);
            let mut child = command.spawn()?;
            let status = match tokio::time::timeout(Duration::from_secs(30), child.wait()).await {
                Ok(status) => status?,
                Err(_) => {
                    let _ = child.kill().await;
                    anyhow::bail!("comando excedió 30 segundos");
                }
            };
            anyhow::ensure!(status.success(), "comando terminó con {status}");
        }
        ActionType::Notify | ActionType::None => {}
    }
    Ok(())
}
