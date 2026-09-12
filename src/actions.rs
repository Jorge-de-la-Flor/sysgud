//! Ejecutor de acciones (`ActionRunner` en la propuesta original).
//!
//! Toma la [`crate::core::AgentAction`] decidida por el motor de
//! decisiones y la aplica en el sistema operativo.

pub mod runner;

pub use runner::execute;
