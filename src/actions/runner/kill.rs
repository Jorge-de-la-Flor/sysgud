use anyhow::{anyhow, Result};
use colored::*;

/// Termina el proceso supervisado por PID.
pub async fn run(target_pid: Option<u32>) -> Result<()> {
    let Some(pid) = target_pid else {
        return Err(anyhow!(
            "se solicitó KILL pero no había un PID de destino disponible"
        ));
    };

    println!("Terminating PID {}", pid.to_string().red().bold());

    #[cfg(unix)]
    {
        tokio::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .output()
            .await?;
    }

    #[cfg(windows)]
    {
        tokio::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .await?;
    }

    Ok(())
}
