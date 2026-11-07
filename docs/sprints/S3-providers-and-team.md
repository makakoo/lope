# S3 — Providers and team

[Sprints](README.md) · Previous: [S2](S2-supervised-runtime-and-completion.md) · Next: [S4](S4-single-shot-review-and-memory.md)

## Goal

Port every validator source with Python's exact invocation, and make `team` and `configure` native.

## Preconditions

`docs/evidence/s2-runtime.txt` exists.

## Tasks

1. **Built-ins and presets** (FR15). Per-validator argv, stdin and env construction is ported from `validators.py`, `cli_discovery.py` and `generic_validators.py`, including output extraction (opencode JSON events, gemini JSON).
2. **Generic subprocess and HTTP providers** (FR16). HTTP is Tier 1 on a shared `reqwest` client (rustls, `default-features = false`), with `{prompt}` substituted through `serde_json::Value`.
3. **Makakoo adapter validators** (FR17) and **learned adapters / self-heal** (FR18).
4. **`--from-curl` parser** (FR19), validated against `tests/test_curl_parser.py`. Native `team list|enable|disable|add|remove|test|health`.
5. **Interactive `configure`**, `latency.json`, retry policy, request plan and budgets (FR20).

## Acceptance

- A8 passes; `team` goldens match Python; the NFR6 rollback check passes after `team add/remove`.
- Evidence: `docs/evidence/s3-providers.txt`.
