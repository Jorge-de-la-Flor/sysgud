use anyhow::{anyhow, Result};
use colored::*;

/// Ejecuta el comando de remediación devuelto por el agente.
pub async fn run(command: Option<&str>) -> Result<()> {
    let Some(cmd) = command else {
        return Err(anyhow!(
            "se solicitó EXECUTE pero no se proporcionó ningún comando"
        ));
    };

    println!("Executing remediation command: {}", cmd.yellow());

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .await?;

    if !output.stdout.is_empty() {
        println!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr).red());
    }

    Ok(())
}
