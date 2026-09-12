//! Ingesta y ventana de contexto (módulo "Monitor" de la arquitectura).
//!
//! Agrupa el lector asíncrono del proceso objetivo y el buffer circular
//! que conserva las últimas N líneas de log hasta el momento del fallo.

pub mod buffer;
pub mod reader;

pub use buffer::RingBuffer;
pub use reader::{spawn, ReaderHandle};

/// Palabras clave que disparan el análisis del agente al aparecer en un log.
pub const TRIGGER_KEYWORDS: [&str; 3] = ["CRITICAL", "ERROR", "PANIC"];

/// Determina si una línea de log representa una condición de falla que
/// amerita invocar al motor de decisiones.
pub fn is_trigger(line: &str) -> bool {
    TRIGGER_KEYWORDS.iter().any(|keyword| line.contains(keyword))
}
