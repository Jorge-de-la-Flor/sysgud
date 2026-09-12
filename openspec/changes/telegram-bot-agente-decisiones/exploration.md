## Exploration: Telegram bot module communicating with the decision agent

### Current State

sysgud is a single-crate Rust (edition 2021) async daemon: `src/main.rs` is a thin
`#[tokio::main]` that calls `sysgud::run()` in `src/lib.rs`. `run()` wires three
domain modules in a single-shot pipeline: `monitor` (spawn target process, merge
stdout/stderr into an `unbounded_channel`, push lines into a `RingBuffer`) ->
on `is_trigger()` keyword match (`CRITICAL`/`ERROR`/`PANIC`) builds an
`AgentRequest` from `buffer.snapshot()` -> `agent::AgentClient::analyze()` POSTs
to Anthropic Messages API via `reqwest 0.11` -> `actions::execute()` dispatches
the typed `AgentAction` (`KILL`/`EXECUTE`/`NOTIFY`/`None`) and then `break`s out
of the loop. There is zero Telegram/bot code: a repo-wide grep for
`telegram|Telegram|teloxide|chat_id|BotApi` returns no matches. The only
"notification" path is `src/actions/runner/notify.rs`, which just `println!`s the
diagnosis to the local console. `AgentClient` already establishes the reusable
pattern for external HTTP integration: owned `reqwest::Client`, `Option<String>`
secret from env, `analyze()` that never propagates transport errors but degrades
to `NOTIFY` with the failure reason embedded in `diagnosis`.

### Affected Areas

- `src/lib.rs` — orchestration (`run()`); any bot polling loop or notify fan-out must be wired here; current `break` after first event conflicts with a long-lived bot.
- `src/core/config.rs` — `Config::from_env()`; new `TELEGRAM_BOT_TOKEN` / `TELEGRAM_CHAT_ID` (naming TBD) belong here following the existing optional-env-with-default convention.
- `src/core/types.rs` — `AgentAction` / `ActionType`; a Telegram payload (`chat_id`, text formatting) would extend or wrap these types.
- `src/core/error.rs` — `SysgudError`; a `Telegram(String)` variant fits the existing `Agent(String)` / `Action(String)` pattern.
- `src/agent/client.rs` — reference implementation for outbound HTTPS via `reqwest` with graceful-degradation; the Telegram `sendMessage` POST reuses this shape.
- `src/actions/runner/notify.rs` — current console-only sink; natural seam for a Telegram outbound send (or a dispatcher between console and Telegram).
- `src/actions/runner.rs` — `execute()` dispatch; inbound bot commands (e.g. approve `KILL`/`EXECUTE`) would need a new entry point alongside this.
- `Cargo.toml` — currently `tokio(full)`, `reqwest 0.11/json`, `serde`, `thiserror`, `colored`, `anyhow`; a bidirectional bot adds either `teloxide` (new major dep) or raw `reqwest` long-polling against `getUpdates`.
- `.env.example` / `.env` — new Telegram variables must be documented here; currently only Anthropic/model/context/target vars exist.
- Build environment — `cargo test`/`clippy` are currently blocked: `openssl-sys` (via `reqwest`) needs `pkg-config` + `libssl-dev`, neither installed (verified: `pkg-config` missing). Strict TDD is enabled but cannot execute until deps are fixed.

### Approaches

1. **Outbound-only Telegram notifier (extend `notify.rs`)** — add a small `TelegramNotifier` (or extend `notify::run`) that POSTs the diagnosis to `https://api.telegram.org/bot<token>/sendMessage` with `reqwest`, reusing the `AgentClient` error-degradation pattern; config via `TELEGRAM_BOT_TOKEN`/`TELEGRAM_CHAT_ID`.
   - Pros: minimal diff (fits 400-line budget); no new dependencies; follows existing patterns; unit-testable pure pieces (message formatting, URL building); never breaks the daemon (fallback to console).
   - Cons: one-way only (no commands, no approval-gating from chat); does not satisfy "communicate with the decision agent" if bidirectional intent is meant.
   - Effort: Low

2. **New top-level `telegram` module, bidirectional bot** — `src/telegram.rs` + `src/telegram/client.rs` (send) + `src/telegram/updates.rs` (`getUpdates` long-poll loop in a `tokio::spawn`), wired in `lib.rs::run()` concurrently with the monitor loop; supports `/status`, `/diagnosis`, and approval-gating of `KILL`/`EXECUTE` from chat.
   - Pros: full "bot communicates with the agent" semantics; interactive approvals improve safety of destructive actions; clean module boundary matching `monitor`/`agent`/`actions` convention (`telegram.rs` + `telegram/`).
   - Cons: large scope (likely exceeds 400-line budget => chained PRs required); new failure modes (poll loop vs single-shot `break` conflict, rate limits, chat auth); needs either `teloxide` (heavy new dep + OpenSSL build pain) or hand-rolled long-poll; secret/chat-id provisioning and sender authorization design needed; no existing HTTP-mock test harness.
   - Effort: High

3. **Phased: outbound slice first, inbound slice second (chained PRs)** — PR #1 implements Approach 1; PR #2 adds the poll loop and command handling on top of the proven notifier.
   - Pros: each slice independently reviewable and verifiable within budget; derisks credential plumbing and Telegram API error behavior early; keeps strict TDD feasible (small units first).
   - Cons: two reviews instead of one; interim state is one-way only; requires the orchestrator's Ask-me/chain strategy to be confirmed upfront.
   - Effort: Medium

### Recommendation

Phased Approach 3: ship the outbound notifier (Approach 1) as the first deliverable
because it satisfies the visible need (diagnoses reach Telegram) with the smallest
blast radius, reuses the proven `reqwest` + graceful-degradation + `Config::from_env`
patterns, and fits the 400-line review budget and strict TDD. Scope the bidirectional
command/approval loop (Approach 2) as an explicit follow-up change once token/chat-id
provisioning and the `lib.rs` concurrency redesign (replacing the single-shot `break`)
are agreed. This ordering also unblocks value while `openssl` build deps are being fixed.

### Risks

- `lib.rs::run()` is single-shot (`break` after first trigger); a long-lived bot polling loop requires redesigning the main loop and shutdown semantics.
- Build/test loop is currently broken (missing `pkg-config` + `libssl-dev` for `openssl-sys` via `reqwest 0.11`); strict TDD cannot execute until fixed — any new `reqwest` code inherits this blocker.
- Telegram secrets (`BOT_TOKEN`, `CHAT_ID`) handling: `.env` is committed-adjacent (`.env` mirrors `.env.example` with empty values); token leak and chat authorization (who may approve `KILL`/`EXECUTE`) need explicit decisions.
- No existing Telegram/bot crates: adding `teloxide` pulls significant new dependencies; raw `reqwest` long-poll avoids that but means hand-rolling `getUpdates` offset management and retry/backoff.
- Telegram API failures must follow the `AgentClient` never-crash contract (degrade to console `NOTIFY`); rate limiting and chat-id misconfiguration need defined behavior.
- Scope ambiguity: "interacture con un bot ... para que se comunique con el agente de decisiones" does not specify direction (outbound alerts vs inbound commands vs approval gate) — proposal must pin this before spec.
- `openspec/` was never bootstrapped (sdd-init persisted engram-only per its hard rule); this exploration wrote `openspec/changes/telegram-bot-agente-decisiones/exploration.md` to honor the hybrid Both-artifacts preflight — orchestrator should confirm whether `openspec/config.yaml` bootstrap is now desired.

### Ready for Proposal

Yes — with three clarifications the orchestrator should get from the user: (1) direction:
outbound alerts only, inbound commands, or both (approval-gating?); (2) credential
provisioning: who creates the bot token and which chat(s) receive messages, plus who is
authorized to trigger actions; (3) delivery: confirm phased chained-PR strategy under
the 400-line budget given strict TDD is blocked on `openssl` deps.
