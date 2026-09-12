# telegram-approvals Specification

## Purpose

Inbound `getUpdates` commands and approval gate blocking `KILL`/`EXECUTE` until allowlisted `/approve`.

## Requirements

### Requirement: Poll Loop

The system MUST poll `GET /bot<token>/getUpdates?offset=&timeout=` in a `tokio::spawn` task concurrent with monitor when enabled, tracking `offset = last_update_id+1` with backoff. MUST replace single-shot `break` in `lib.rs::run()` with long-lived loop.

#### Scenario: Update processed
- GIVEN polling with offset `N`
- WHEN `getUpdates` returns `update_id=N`
- THEN update handled and next offset is `N+1`

#### Scenario: Disabled no task
- GIVEN `TELEGRAM_BOT_TOKEN` unset
- WHEN `run()` starts
- THEN no `getUpdates` task spawned

### Requirement: Command Parsing

The system MUST parse `/status`, `/approve`, `/reject` (allow `/status@bot` suffix). Unknown text MUST be ignored. Non-allowlisted senders MUST be ignored.

#### Scenario: /status reply
- GIVEN pending `KILL` and allowlisted sender
- WHEN `/status` received
- THEN system replies with pending action and diagnosis

#### Scenario: Non-allowlisted ignored
- GIVEN sender `999` not in allowlist
- WHEN `/approve` from `999`
- THEN approval denied; pending unchanged

### Requirement: Approval Gate

The system MUST block `Kill`/`Execute` until allowlisted `/approve` received. `Notify`/`None` MUST execute immediately without gate.

#### Scenario: KILL blocked
- GIVEN agent returns `KILL`
- WHEN no `/approve` yet
- THEN `kill::run` NOT called; pending stored and re-announced

#### Scenario: /approve unblocks
- GIVEN `KILL` pending blocked
- WHEN allowlisted `/approve` received
- THEN `kill::run(target_pid)` executes and pending cleared

#### Scenario: NOTIFY bypasses gate
- GIVEN agent returns `NOTIFY`
- WHEN gate evaluated
- THEN delivered via Telegram or console without waiting

### Requirement: Reject

The system MUST clear pending on allowlisted `/reject` and reply with cancellation; no remediation executed.

#### Scenario: /reject cancels
- GIVEN `KILL` pending
- WHEN allowlisted `/reject` received
- THEN pending cleared; `kill::run` not called

### Requirement: Allowlist Authorization

The system MUST enforce `TELEGRAM_ALLOWLIST` (comma-separated IDs) from env. Empty allowlist MUST deny all `/approve`/`/reject`. Identity MUST be `message.from.id`.

#### Scenario: Allowlist match
- GIVEN `TELEGRAM_ALLOWLIST=111,222`
- WHEN `/approve` from `111`
- THEN accepted; from `333` rejected
