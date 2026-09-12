//! Binario delgado de sysgud.
//!
//! Toda la lógica vive en módulos de librería (`src/core`, `src/monitor`,
//! `src/agent`, `src/actions`) y en la orquestación de `src/lib.rs`. Este
//! archivo solo arranca el runtime async y delega el control.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sysgud::run().await
}
