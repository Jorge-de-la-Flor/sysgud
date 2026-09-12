# telegram-notify Specification

## Purpose

Outbound `sendMessage` delivery of diagnoses to Telegram; degrades to console when disabled or on failure. `reqwest`-only.

## Requirements

### Requirement: Outbound Delivery

The system MUST POST diagnosis to `https://api.telegram.org/bot<token>/sendMessage` with `chat_id` and text when `TELEGRAM_BOT_TOKEN` and `TELEGRAM_CHAT_ID` are set.

#### Scenario: Diagnosis reaches Telegram
- GIVEN trigger detected and `TELEGRAM_*` set
- WHEN `NOTIFY` executes
- THEN `sendMessage` is called with `chat_id` and diagnosis

#### Scenario: Disabled when unconfigured
- GIVEN `TELEGRAM_BOT_TOKEN` or `TELEGRAM_CHAT_ID` empty
- WHEN diagnosis dispatched
- THEN no HTTP call; console output only

### Requirement: Graceful Degrade

The system MUST NOT propagate Telegram errors. On network/4xx/5xx/429/timeout it MUST fallback to console `NOTIFY`.

#### Scenario: API failure fallback
- GIVEN `sendMessage` fails
- WHEN diagnosis dispatched
- THEN system prints to console and returns `Ok` with failure reason

### Requirement: Configuration

The system MUST read `TELEGRAM_BOT_TOKEN` (`Option<String>`), `TELEGRAM_CHAT_ID` (`Option<String>`), `TELEGRAM_ALLOWLIST` (comma-separated IDs) via `Config::from_env()`.

#### Scenario: Env parsed
- GIVEN `TELEGRAM_BOT_TOKEN=abc` and `TELEGRAM_CHAT_ID=123`
- WHEN `Config::from_env()` called
- THEN fields are `Some`

### Requirement: Client and Dependencies

The system MUST use owned `reqwest::Client` per `AgentClient` pattern and MUST NOT add `teloxide`.

#### Scenario: No teloxide
- GIVEN `Cargo.toml` inspected
- WHEN checking dependencies
- THEN `teloxide` absent; `TelegramClient` uses `reqwest::Client`

### Requirement: Error Variant

The system MUST add `SysgudError::Telegram(String)` in `src/core/error.rs`.

#### Scenario: Variant formats
- GIVEN `SysgudError::Telegram("x")`
- WHEN displayed
- THEN message contains `telegram error`
