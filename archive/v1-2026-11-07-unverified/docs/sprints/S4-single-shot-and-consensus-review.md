# Sprint S4: Single-Shot Verbs (`ask`/`vote`/`compare`/`pipe`) & Consensus Review (`review`)

[Sprints Index](README.md) · Previous: [S3 — Validators & HTTP Engine](S3-validators-and-http-engine.md) · Next: [S5 — Memory & Makakoo Bridge](S5-memory-and-makakoo-bridge.md)

---

## Goal

Implement the single-shot fan-out verbs (`ask`, `vote`, `compare`, `pipe`) and the complete consensus review pipeline (`lope review <file> --consensus`, finding extraction, fuzzy deduplication, quorum scoring, 7 Glimmer Role Lenses `--focus`, atomic `--output <path>`, and SARIF 2.1.0 export) in `lope-engine` (`A9`, `A10`, `SC7`).

## Preconditions

- `docs/evidence/s3-validators.txt` exists on disk.

## Implementation Tasks

1. **Single-Shot Fan-Out Verbs (`FR17`)**:
   - **`lope ask <prompt>`**: Concurrent fan-out across enabled validators using `tokio::task::JoinSet` (bounded by `max_parallel_ptys` semaphore for PTY validators, unbounded for HTTP validators). Supports `--timeout`, `--json`, `--markdown`, `--require-all`.
   - **`lope vote <question> --options <csv>`**: Parse normalized option labels from validator responses, tally picks, and print winner + per-validator breakdown.
   - **`lope compare <fileA> <fileB> --criteria <text>`**: Enforce mandatory `--criteria`, randomize A/B presentation order if configured, and tally picks.
   - **`lope pipe`**: Read `stdin` to EOF, enforce `max_input_bytes`, and fan out with per-validator error isolation.
2. **Consensus Review Engine & 7 Role Lenses (`FR18`–`FR22`)**:
   - Port `review.py` and `findings.py` into `lope-engine::review`.
   - Support single positional `<file>` argument and `--focus <lens>` (mapping shorthand keys `product`, `sales`, `tax`, `finance`, `legal`, `research`, `engineering` and full Glimmer lens titles to specialized review prompts).
   - Extract structured findings (`- [HIGH|MEDIUM|LOW|CRITICAL] <file>:<line> — <message> (confidence: 0.XX)`), merge overlapping findings across validators (`merge_findings` by file, `±5` line proximity, and normalized token Jaccard similarity), and score by consensus mode (`quorum`, `unanimous`, `any`).
   - Render in all 5 formats: `text`, `json`, `markdown`, `markdown-pr`, and `sarif` (SARIF 2.1.0 schema in `lope-memory::sarif`).
   - Write `--output <path>` atomically (`0644`, ANSI-stripped, secret-redacted) while streaming progress to terminal.

## Acceptance Gate & Evidence (`docs/evidence/s4-review.txt`)

- `A9` passes: `ask`, `vote`, `compare`, and `pipe` fan-out and partial-failure isolation tests pass.
- `A10` & `SC7` pass: `lope review` with `--consensus quorum --format markdown --focus tax --output <path>` and `--format sarif` produces verified deduplicated findings and valid SARIF 2.1.0 JSON.
- Record test results in `docs/evidence/s4-review.txt`.

---

[Sprints Index](README.md) · Previous: [S3 — Validators & HTTP Engine](S3-validators-and-http-engine.md) · Next: [S5 — Memory & Makakoo Bridge](S5-memory-and-makakoo-bridge.md)
