

/// Comandos que un usuario de Telegram puede enviar al bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Consultar el estado de la aprobación pendiente.
    Status,
    /// Aprobar la acción almacenada (Kill o Execute).
    Approve,
    /// Rechazar la acción pendiente.
    Reject,
    /// Texto no reconocido como comando.
    Unknown,
}

/// Parsea el texto de un mensaje de Telegram en un `Command`.
///
/// - Stripea el sufijo `@bot` del texto (ej: `/approve@bot` → `Approve`, `/approve@bot extra` → `Approve`)
/// - `/status` → `Status`
/// - `/approve` → `Approve`
/// - `/reject` → `Reject`
/// - Cualquier otro texto → `Unknown`
pub fn parse(text: &str) -> Command {
    let stripped = text.split('@').next().unwrap_or(text);
    match stripped.trim() {
        "/status" => Command::Status,
        "/approve" => Command::Approve,
        "/reject" => Command::Reject,
        _ => Command::Unknown,
    }
}

/// Determina si un remitente está en la allowlist.
///
/// Regla de deny-all: si la allowlist está vacía, ningún remitente
/// está autorizado. Esto es la política de seguridad por defecto.
pub fn is_allowlisted(from_id: i64, allowlist: &[String]) -> bool {
    if allowlist.is_empty() {
        return false;
    }
    allowlist
        .iter()
        .any(|id| id.parse::<i64>().map(|x| x == from_id).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_approve_with_bot_suffix() {
        assert_eq!(parse("/approve@bot extra"), Command::Approve);
    }

    #[test]
    fn parse_status() {
        assert_eq!(parse("/status"), Command::Status);
    }

    #[test]
    fn parse_reject() {
        assert_eq!(parse("/reject"), Command::Reject);
    }

    #[test]
    fn parse_unknown() {
        assert_eq!(parse("hello"), Command::Unknown);
    }

    #[test]
    fn parse_allowlisted_sender() {
        assert!(is_allowlisted(111, &["111".to_string(), "222".to_string()]));
    }

    #[test]
    fn parse_not_allowlisted() {
        assert!(!is_allowlisted(333, &["111".to_string(), "222".to_string()]));
    }

    #[test]
    fn parse_empty_allowlist_deny_all() {
        assert!(!is_allowlisted(111, &[]));
    }
}