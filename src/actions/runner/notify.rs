use anyhow::Result;
use colored::*;

/// Solo reporta el diagnóstico en consola, sin tocar el proceso supervisado.
pub fn run(diagnosis: &str) -> Result<()> {
    println!("{}", "[+] Alert dispatched to system console.".green());
    println!("    {}", diagnosis);
    Ok(())
}
