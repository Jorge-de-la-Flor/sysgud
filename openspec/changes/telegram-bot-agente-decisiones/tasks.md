# Tasks: Telegram Bot for Decision Agent

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~188 (PR #1), ~310 (PR #2) |
| 400-line budget risk | Low |
| Chained PRs recommended | Yes |
| Suggested split | PR #1 → PR #2 |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main |
| 400-line budget risk | Low |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Low

### Suggested Work Units

| Unit | Goal | Likely PR | Focused test command | Runtime harness | Rollback boundary |
|------|------|-----------|----------------------|-----------------|-------------------|
| 1 | Outbound sendMessage + config + error + notify seam | PR #1 | `cargo test --lib telegram_notify` | Env-gated unit tests; no HTTP needed | `src/telegram/client.rs` + config/error/notify changes are independently revertable |
| 2 | Inbound poll loop + command parser + approval gate | PR #2 | `cargo test --lib telegram_approvals` | Env-gated integration tests with mocked send() | `src/telegram/updates.rs` + `commands.rs` + runner gate are independently revertable |

---

## Phase 1: PR #1 — telegram-notify (Outbound sendMessage behind notify.rs seam)

### Foundation

- [x] 1.1 Create `src/telegram/mod.rs` with `mod client`, `pub fn is_enabled(config: &Config) -> bool` checking `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID` are both `Some`
- [x] 1.2 Create `src/telegram/client.rs` with `TelegramClient { http: reqwest::Client, token: String, chat_id: String }`, `pub fn new(token, chat_id) -> Self`, and `pub async fn send(&self, text: &str) -> Result<(), SysgudError>` POSTing to `https://api.telegram.org/bot<token>/sendMessage`

### Core type changes

- [x] 2.1 Modify `src/core/config.rs` — add `pub telegram_bot_token: Option<String>`, `pub telegram_chat_id: Option<String>`, `pub telegram_allowlist: Vec<String>` fields; parse from `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID`, `TELEGRAM_ALLOWLIST` (CSV split, trim) in `Config::from_env()`
- [x] 2.2 Modify `src/core/error.rs` — add `#[error("telegram error: {0}")] Telegram(String)` variant to `SysgudError`

### Notify seam integration

- [x] 3.1 Modify `src/actions/runner/notify.rs` — `run(diagnosis: &str)` checks `Config::from_env()`, if `is_enabled()` constructs `TelegramClient` and calls `send(diagnosis)`; on any error returns `Ok(())` after printing to console (graceful degrade)
- [x] 3.2 Modify `.env.example` — add `TELEGRAM_BOT_TOKEN=`, `TELEGRAM_CHAT_ID=`, `TELEGRAM_ALLOWLIST=` documentation lines

### Tests

- [x] 4.1 Unit test `TelegramClient::send` URL construction and request body (mock `reqwest::Client` or test URL building logic in isolation)
- [x] 4.2 Unit test `Config::from_env()` parses `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID`, `TELEGRAM_ALLOWLIST` correctly (including CSV split/trim)
- [x] 4.3 Unit test `is_enabled()` returns `true` when both token and chat_id are `Some`, `false` when either is `None`
- [x] 4.4 Integration test: unconfigured env → `notify::run` produces console output only, no HTTP call
- [x] 4.5 Integration test: `SysgudError::Telegram("x")` formatted message contains `telegram error`

---

## Phase 2: PR #2 — telegram-approvals (Inbound poll loop + parser + runner gate)

### Module extension

- [x] 1.1 Extend `src/telegram/mod.rs` — add `mod commands`, `mod updates`, re-export `TelegramCtx`, `Command`, `parse()`, `is_allowlisted()`; define `pub struct TelegramCtx { pub client: Option<TelegramClient>, pub pending: Arc<Mutex<Option<PendingApproval>>>, pub allowlist: Vec<String> }`

### New files: updates + commands

- [x] 2.1 Create `src/telegram/commands.rs` — `pub fn parse(text: &str) -> Command` stripping `@bot` suffix, matching `/status`, `/approve`, `/reject`, `Unknown` otherwise; `pub fn is_allowlisted(from_id: i64, allowlist: &[String]) -> bool` (empty allowlist = deny-all)
- [x] 2.2 Create `src/telegram/updates.rs` — `pub async fn spawn_poll(client: TelegramClient, ctx: Arc<Mutex<TelegramCtx>>)` loop: GET `getUpdates?offset=&timeout=30`, on 200 parse `result` array, process each update, set `offset = last_update_id + 1`, backoff on 429/net errors; runs in `tokio::spawn`; bounded by `timeout=30s`

### Core type changes

- [x] 3.1 Modify `src/core/types.rs` — add `pub struct PendingApproval { pub action: AgentAction, pub pid: Option<u32> }` and `pub struct PendingStore(Arc<Mutex<Option<PendingApproval>>>)` if useful

### lib.rs orchestration rewrite

- [x] 4.1 Modify `src/lib.rs` — add `pub mod telegram`; build shared `pending: Arc<Mutex<Option<PendingApproval>>>` and `TelegramCtx`; when `is_enabled()`, `tokio::spawn(poll_loop(...))` concurrent with monitor loop; remove `break` after `run_action()` — loop until child exits or `ctrl_c` signal; add `tokio::signal::ctrl_c()` handler to abort poll task and exit gracefully

### Runner gate

- [x] 5.1 Modify `src/actions/runner.rs` — change `execute()` signature to accept `&TelegramCtx`; add gate before `ActionType::Kill`/`ActionType::Execute`: if `pending` is `Some` and no approved, store `PendingApproval { action, pid }`, announce via `TelegramClient::send()`, return `Ok(())` without calling `kill::run`/`execute_cmd::run`; `ActionType::Notify`/`None` bypass gate and execute immediately
- [x] 5.2 On allowlisted `/approve`: extract stored `PendingApproval`, call `kill::run(pid)` or `execute_cmd::run(command)`, clear pending; on allowlisted `/reject`: clear pending, reply cancelled

### Tests

- [x] 6.1 Unit test `parse("/approve@bot extra")` returns `Command::Approve`
- [x] 6.2 Unit test `parse("hello")` returns `Command::Unknown`
- [x] 6.3 Unit test `parse("/status")` returns `Command::Status`
- [x] 6.4 Unit test `is_allowlisted(111, &["111,222"])` returns `true`
- [x] 6.5 Unit test `is_allowlisted(333, &["111,222"])` returns `false` (empty allowlist = deny-all)
- [x] 6.6 Unit test offset management: after processing `update_id=N`, next offset is `N+1`
- [x] 6.7 Integration test: `KILL` blocked when pending is `Some` and no approval; `/approve` from allowlisted sender executes stored action and clears pending
- [x] 6.8 Integration test: `/reject` from allowlisted sender clears pending, no action executed
- [x] 6.9 Integration test: `NOTIFY` bypasses gate and delivers immediately without waiting

---

## Phase 3: Cross-PR Verification

- [x] 3.1 Fix `openssl-sys` / `pkg-config` / `libssl-dev` dependency blocker to enable `cargo test` and `cargo clippy`
- [x] 3.2 Run `cargo test --lib` — all unit and integration tests pass for both PRs
- [x] 3.3 Run `cargo clippy --all-targets` — zero warnings
- [x] 3.4 Verify PR #1 independently: empty `TELEGRAM_*` env vars → console-only path works unchanged
- [x] 3.5 Verify PR #2 independently: no `TELEGRAM_BOT_TOKEN` → no poll task spawned, `lib.rs::run()` behaves as before

---

## Phase 4: Cleanup & Documentation

- [x] 4.1 Update `src/telegram/mod.rs` doc comments with module-level documentation
- [x] 4.2 Add inline doc comments to `TelegramClient::send()`, `spawn_poll()`, `parse()`, gate logic
- [x] 4.3 Remove any temporary test fixtures or mock scaffolding from PR #2 if unused
- [x] 4.4 Verify `.env.example` documents all three `TELEGRAM_*` vars with descriptions

---

## Dependency Map

```
PR #1:
  1.1 ──┐
  1.2 ──┤
  2.1 ──┤── 3.1 (notify.rs needs Config fields)
  2.2 ──┤
  3.2 ──┘ (independent)
  4.1-4.5 (tests depend on 1-3)

PR #2:
  1.1 (depends on PR #1's mod.rs + client.rs + types.rs)
  2.1 (depends on 1.1)
  2.2 (depends on 1.1)
  3.1 (independent of PR #2's other tasks)
  4.1 (depends on 1.1, 3.1, lib.rs structure)
  5.1 (depends on 1.1, 3.1, 4.1)
  5.2 (depends on 2.1, 5.1)
  6.1-6.9 (tests depend on 1-5)
```
