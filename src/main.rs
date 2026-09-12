//! Binario delgado de sysgud.
//!
//! La lógica vive en los crates del workspace y en `src/lib.rs`. Este
//! archivo solo arranca el runtime async y delega el control.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    sysgud::run().await
}
