# Sprint S3: Native HTTP Engine, 15 CLI Adapters, Curl Parser & Team CLI

[Sprints Index](README.md) · Previous: [S2 — Warm Pool & Daemon](S2-warm-pool-and-daemon.md) · Next: [S4 — Single-Shot & Consensus Review](S4-single-shot-and-consensus-review.md)

---

## Goal

Implement `lope-providers`: the **Tier 1 Native Async HTTP Engine** (`reqwest` + `rustls-tls`), all 15 built-in CLI validator adapters + generic subprocess/HTTP providers, `--from-curl` parser, `lope team` / `lope configure` CLIs, and budget/lease/retry orchestration (`A7`, `A8`, `SC6`).

## Preconditions

- `docs/evidence/s2-daemon-pool.txt` exists on disk.

## Implementation Tasks

1. **Tier 1 Native Async HTTP Validator (`FR13`)**:
   - Build `GenericHttpValidator` and `SwitchAiLocalAdapter` using a shared `reqwest::Client` (`rustls-tls`, connection pooling, configurable timeout).
   - Substitute `{prompt}` in JSON body templates safely via `serde_json::Value` traversal (never raw string concatenation that could break JSON escaping) and resolve `${ENV_VAR}` only in headers/body (`NFR4`).
   - Extract responses using dotted JSON paths (`choices.0.message.content`, `content.0.text`, etc.).
2. **15 Built-In CLI Adapters & Generic Subprocess Validator (`FR14`, `FR15`)**:
   - Port built-in presets (`claude`, `codex`, `gemini`, `opencode`, `aider`, `grok`, `agy`, `kimi`, `qwen`, `pi`, `vibe`, `cursor`, `copilot`, `ollama`, `goose`) plus custom `"type": "subprocess"` entries from `~/.lope/config.json`.
   - Route each CLI call through `loped` Tier 3 Warm Pool when available (`C2`), or `lope-runtime-pty` Tier 2 One-Shot PTY (`C1`), with automatic `0600` temp prompt file handling when prompt bytes approaches `ARGV_POLICY_LIMIT` (`128 KB`).
3. **`--from-curl` Parser & `lope team` CLI (`FR16`)**:
   - Port `curl_parser.py` to Rust: parse `curl` flags (`-X`, `-H`, `--header`, `-d`, `--data`, `--data-raw`, `--data-binary`, `--url`), infer `response_path`, and register the provider in `~/.lope/config.json`.
   - Implement `lope team {list,add,remove,test,enable,disable}` and `lope configure`.
4. **Budget Leases, Admission Planner & Retry Policy (`FR7`, `FR8`)**:
   - Port `request_plan.py`, `invocation.py`, and `retry_policy.py` into `lope-core::invocation`.
   - Enforce `max_calls`, `max_chunks`, `max_input_bytes`, and `remaining_seconds` leases, plus single transient retry with deterministic jitter for `rate_limited` / `service_unavailable`.

## Acceptance Gate & Evidence (`docs/evidence/s3-validators.txt`)

- `A7` & `SC6` pass: `lope team add` with `--from-curl` registers a provider and `lope team test` succeeds against a local `axum` mock server with `0` subprocesses spawned.
- `A8` passes: Budget exhaustion, single transient retry, and secret redaction tests pass 100%.
- Record test results in `docs/evidence/s3-validators.txt`.

---

[Sprints Index](README.md) · Previous: [S2 — Warm Pool & Daemon](S2-warm-pool-and-daemon.md) · Next: [S4 — Single-Shot & Consensus Review](S4-single-shot-and-consensus-review.md)
