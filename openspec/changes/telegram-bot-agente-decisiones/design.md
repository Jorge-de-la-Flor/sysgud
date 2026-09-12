# Design: Telegram Bot for Decision Agent

## Technical Approach

Bidirectional Telegram bridge in two chained PRs. PR #1 adds `src/telegram/client.rs` (reqwest `sendMessage`) behind the `notify.rs` seam with console degrade. PR #2 adds `updates.rs` long-poll loop (`tokio::spawn`, concurrent with monitor), `commands.rs` parser, shared pending-approval store, and gate in `actions/runner.rs`. Maps to `telegram-notify` (outbound, degrade) and `telegram-approvals` (poll, parse, gate, reject, allowlist) specs. No new deps; follows `AgentClient` owned-`reqwest::Client` pattern.

## Architecture Decisions

| Option | Tradeoff | Decision |
|---|---|---|
| reqwest vs teloxide | teloxide ergonomic but new dep + API surface | reqwest-only, reuse `AgentClient` pattern |
| `tokio::spawn` poll vs inline `select!` in `run()` | inline blocks monitor line intake during long-poll timeout | Spawned task, shares `Arc<Mutex<Pending>>` + `TelegramClient` |
| Shutdown via `JoinHandle::abort` + `tokio::signal::ctrl_c` vs `CancellationToken` | Token needs `tokio-util` (new dep) | `ctrl_c` + abort on `run()` exit; long-poll `timeout=30s` bounds staleness |
| Gate inside `runner::execute` vs caller in `lib.rs` | Caller gate splits dispatch logic across files | Gate in `execute()` before `Kill`/`Execute` arms; `Notify`/`None` bypass |
| Long-lived monitor loop vs single-shot `break` | `break` kills daemon after first event | Remove `break`; loop until child exits or `ctrl_c` |

## Data Flow

    monitor lines ──→ trigger ──→ agent.analyze ──→ runner::execute ──→ TelegramClient.send ──→ chat
         │                              │                      │                              │
         │                              │                      └──── pending store ───────────┘
         │                              │                           ▲            │
    getUpdates poll task ──→ commands::parse ──→ allowlist check ──┘      /approve → kill/execute
                                                                              /reject → clear

`offset = last_update_id + 1` per poll; `sendMessage` failures return `Ok` after console fallback.

## File Changes

| File | Action | Description |
|---|---|---|
| `src/telegram/mod.rs` | Create | Re-exports; `is_enabled()` (token+chat set) |
| `src/telegram/client.rs` | Create | `TelegramClient { http: reqwest::Client, token, chat_id }`; `send(text)` POSTs `sendMessage`, maps net/429/5xx to degrade |
| `src/telegram/updates.rs` | Create | `spawn_poll(client, pending, allowlist)`; GET `getUpdates?offset=&timeout=30`, backoff on 429/net err |
| `src/telegram/commands.rs` | Create | `parse(text)` strips `@bot` suffix, matches `/status│/approve│/reject`; `is_allowlisted(from_id)`; empty allowlist = deny-all |
| `src/lib.rs` | Modify | Declare `mod telegram`; build shared `pending: Arc<Mutex<Option<PendingApproval>>>`; spawn poll when enabled; remove `break` |
| `src/actions/runner.rs` | Modify | `execute()` takes `&TelegramCtx`; `Kill`/`Execute` check pending gate, store + announce if unapproved |
| `src/actions/runner/notify.rs` | Modify | Try `TelegramClient::send` first, fallback to existing console print |
| `src/core/config.rs` | Modify | `TELEGRAM_BOT_TOKEN/CHAT_ID: Option<String>`, `TELEGRAM_ALLOWLIST: Vec<String>` (CSV split, trim) via `from_env()` |
| `src/core/error.rs` | Modify | Add `SysgudError::Telegram(String)` (`#[error("telegram error: {0}")]`) |
| `src/core/types.rs` | Modify | Add `PendingApproval { action: AgentAction, pid: Option<u32> }` |
| `Cargo.toml` | Unchanged | reqwest/tokio/serde already present; teloxide forbidden |
| `.env.example` | Modify | Document three `TELEGRAM_*` vars |

## Interfaces / Contracts

```rust
pub struct TelegramCtx { pub client: Option<TelegramClient>, pub pending: Arc<Mutex<Option<PendingApproval>>>, pub allowlist: Vec<String> }
impl TelegramClient { pub fn new(token: String, chat_id: String) -> Self; pub async fn send(&self, text: &str) -> Result<(), SysgudError>; }
pub enum Command { Status, Approve, Reject, Unknown }
pub fn parse(text: &str) -> Command; // "/approve@bot extra" -> Approve
```

Gate rule: `Kill|Execute` + no approved pending → store `PendingApproval`, announce via Telegram, return `Ok(())` without `kill::run`/`execute_cmd::run`. Allowlisted `/approve` → run stored action once, clear. `/reject` → clear, reply cancelled.

## Testing Strategy

| Layer | What to Test | Approach |
|---|---|---|
| Unit | `parse` (+`@bot` suffix, unknown), allowlist (empty=deny, CSV, `from.id` identity), offset `N→N+1` | `#[tokio::test]` pure fns, no network |
| Integration | Unconfigured → no spawn/no HTTP; API fail → console `Ok`; `KILL` blocked→`/approve` executes→`/reject` clears; `NOTIFY` bypasses | Env-gated tests; HTTP mocked at `send()` seam |
| E2E | Manual: diagnosis arrives; `/status` shows pending | Manual against test bot (openssl fixed first) |

## Threat Matrix

N/A — no routing, shell, subprocess, VCS/PR automation, executable-file classification, or process-integration boundary introduced. Gate wraps existing `kill`/`sh -c` dispatch without changing command composition; poll transport is HTTPS GET/POST only.

## Migration / Rollout

No migration. PR #1 ships disabled-by-default (empty `TELEGRAM_*` = console-only). PR #2 enables poll only when token set. Rollback: revert PR #2 then PR #1 independently. Blocker: fix `pkg-config`/`libssl-dev` before strict-TDD `cargo test`/`clippy` enforcement.

## Open Questions

- [ ] Target chat ID + approver IDs still needed from user (token exists)?
- [ ] Confirm ask-me chained-PR split: PR #1 = client+config+error+notify seam; PR #2 = updates+commands+gate+loop?
