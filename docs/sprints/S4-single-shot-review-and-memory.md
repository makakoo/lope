# S4 — Single-shot verbs, review and memory

[Sprints](README.md) · Previous: [S3](S3-providers-and-team.md) · Next: [S5](S5-makakoo-bridge.md)

## Goal

Make the highest-traffic verbs native: `ask`, `vote`, `compare`, `pipe`, `review` (Glimmer G4 and G5) and `memory`. From this sprint, real validator calls run through the S2 runtime, so FR12 diagnostics start accumulating for OQ-4.

## Preconditions

`docs/evidence/s3-providers.txt` exists.

## Tasks

1. **`ask`, `vote`, `compare`, `pipe`** (FR21), with fan-out isolation, `--require-all`, `--synth`, `--anonymous` and `--json`.
2. **`review`** (FR22):
   - single positional and `--divide/--roles`;
   - `--focus` passed verbatim, plus minimality aliases;
   - all formats, `--output/-o` atomic;
   - stdout carries the report (with Python's header lines); progress goes to stderr.
3. **Findings, consensus buckets, SARIF, exporters** (FR24); **verdict repair and events** (FR25); **caveman** (FR26).
4. **`memory.db`** with the exact schema and `user_version = 1`, WAL, busy timeout (FR27, NFR9). Native `memory *`; `review --remember`.
5. Brain flags remain **delegated** (the coverage table excludes them) until S5.

## Acceptance

- A9, A10, A11 (G4, G5) and A12 pass.
- NFR2 fan-out RSS is measured.
- The NFR6 rollback check passes on `memory.db`.
- Start the 7-day FR12 diagnostics collection for OQ-4.
- Evidence: `docs/evidence/s4-review-memory.txt`.
