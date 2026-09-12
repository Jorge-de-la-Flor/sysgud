use crate::core::Config;
use crate::telegram::is_enabled;
use crate::telegram::client::TelegramClient;
use anyhow::Result;
use colored::*;

/// Notifica el diagnóstico al chat de Telegram cuando está
/// habilitado; degrada a consola cuando no lo está o falla
/// cualquier llamada a la API.
pub async fn run(diagnosis: &str) -> Result<()> {
    let config = Config::from_env();

    if is_enabled(&config) {
        let client = TelegramClient::new(
            config.telegram_bot_token.clone().unwrap(),
            config.telegram_chat_id.clone().unwrap(),
        );
        // El resultado se ignora: cualquier fallo de Telegram
        // se degrada a consola sin propagar el error.
        let _ = client.send(diagnosis).await;
    }

    println!("{}", "[+] Alert dispatched to system console.".green());
    println!("    {}", diagnosis);
    Ok(())
}
