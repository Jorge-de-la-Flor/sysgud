use thiserror::Error;

/// Errores de dominio de sysgud. Los puntos de más alto nivel suelen
/// envolver estos casos en `anyhow::Error`, pero mantenerlos tipados aquí
/// facilita el manejo específico cuando hace falta.
#[derive(Debug, Error)]
pub enum SysgudError {
    #[error("no se pudo lanzar el proceso objetivo: {0}")]
    Spawn(#[from] std::io::Error),

    #[error("la llamada al agente falló: {0}")]
    Agent(String),

    #[error("no se pudo interpretar la respuesta del agente: {0}")]
    Parse(#[from] serde_json::Error),

    #[error("la ejecución de la acción falló: {0}")]
    Action(String),
}
