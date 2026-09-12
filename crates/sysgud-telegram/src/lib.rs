//! Telegram polling adapter; all decisions go through the authenticated API.
mod client;
mod commands;
pub use client::Bot;
