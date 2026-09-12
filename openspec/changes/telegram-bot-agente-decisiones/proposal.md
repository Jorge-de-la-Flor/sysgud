# Proposal: Telegram Bot for Decision Agent

## Intent

Connect sysgud decision agent to Telegram bidirectionally: push monitoring alerts/diagnoses to chat and require chat approval before destructive `KILL`/`EXECUTE`. Today `notify.rs` only prints locally — no remote visibility, no safety gate.

User decisions: (1) bidirectional with approval-gating; (2) bot token exists, monitoring agent triggers actions; (3) phased chained PRs confirmed under 400-line budget while TDD is openssl-blocked.

## Scope

### In Scope

- `telegram` module: sendMessage client via existing `reqwest` pattern
- Inbound `getUpdates` poll loop: `/status`, `/approve`, `/reject`
- Approval gate blocking `KILL`/`EXECUTE` until chat approval; `NOTIFY` stays auto
- Config `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID` + sender allowlist

### Out of Scope

- `teloxide` migration, webhooks, multi-chat management UI
- Agent prompt logic or trigger-keyword changes
- Metrics/dashboards

## Capabilities

### New Capabilities

- `telegram-notify`: outbound alerts/diagnosis delivery to Telegram chat
- `telegram-approvals`: inbound commands plus approval-gating for destructive actions

### Modified Capabilities

- None — no `openspec/specs/` baseline exists; `notify` behavior extended via new module

## Approach

PR #1: outbound notifier on `notify.rs` seam, graceful-degrade to console. PR #2: `tokio::spawn` poll loop in `lib.rs`, command parser + pending-approval store, gate in `actions/runner.rs`. `reqwest`-only, no `teloxide`; each slice <400 lines.

## Affected Areas

| Area | Impact | Description |
|------|--------|-------------|
| `src/telegram/` | New | `client.rs` send, `updates.rs` poll, `commands.rs` parse |
| `src/lib.rs` | Modified | Concurrent poll loop; replace single-shot `break` |
| `src/core/config.rs` | Modified | Token, chat-id, allowlist env |
| `src/core/error.rs`, `types.rs` | Modified | `Telegram` variant, approval types |
| `src/actions/runner*.rs` | Modified | Approval gate + Telegram fan-out |
| `Cargo.toml`, `.env.example` | Modified | No new deps; document vars |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| openssl build block (`pkg-config`/`libssl-dev`) | High | Fix deps first; PR #1 validates `cargo test` |
| Single-shot `break` vs long-lived loop | Med | Redesign `run()` with spawn + shutdown |
| Token leak / unauthorized approve | Med | Env-only secret, allowlist, default-deny |
| Telegram failure / rate-limit | Low | Degrade to console `NOTIFY`, backoff retry |

## Rollback Plan

Env-flagged: empty `TELEGRAM_*` disables Telegram, console-only path unchanged. Revert PR #2 then PR #1 independently. No data migration.

## Dependencies

- Bot token (done); target chat ID + approver allowlist needed
- Fix `pkg-config` + `libssl-dev` to unblock strict TDD
- Ask-me chained-PR strategy per 400-line budget

## Success Criteria

- [ ] Trigger diagnosis arrives in Telegram chat; console fallback on API failure
- [ ] `KILL`/`EXECUTE` blocked until `/approve` from allowlisted chat; `/reject` cancels
- [ ] Each PR <400 lines; `cargo test`, `clippy` pass once openssl fixed
