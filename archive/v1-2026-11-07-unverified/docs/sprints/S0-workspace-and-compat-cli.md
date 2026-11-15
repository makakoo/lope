# Sprint S0: Workspace Bootstrap, Ghostty-VT Extraction & Drop-In CLI/Config Parity

[Sprints Index](README.md) · [Master Sprint](../../SPRINT-SUPER-LOPE-V1.md) · Next: [S1 — PTY Supervisor & Detector](S1-pty-supervisor-and-detector.md)

---

## Goal

Initialize the 10-crate Rust `2021` Cargo workspace in `/Users/sebastian/Projects/apps/super-lope`, vendor `crates/ghostty-vt` and `vendor/portable-pty` from `/Users/sebastian/projects/apps/herdr`, implement `lope-core` (domain models, `~/.lope/config.json` + `config.toml` serde loader, canonical secret redaction, and `---VERDICT---` parser/repair), and implement the full 27-subcommand `clap` CLI surface in `lope-cli` (`A1`, `A2`).

## Preconditions

- Live pin table from [`docs/audits/README.md`](../audits/README.md) verified and recorded in `docs/evidence/pins.txt`.

## Implementation Tasks

1. **Workspace & Herdr Terminal Crates**:
   - Create `/Users/sebastian/Projects/apps/super-lope/Cargo.toml` aligning shared dependencies (`tokio 1.40`, `rusqlite 0.32`, `reqwest 0.12`, `clap 4.5`, `ratatui 0.30`, `crossterm 0.29`, `serde 1`, `serde_json 1`, `toml 0.8`, `regex 1.11`, `blake3 1.5`, `sha2 0.10`, `interprocess 2.4.2`, `bincode 2`).
   - Copy `/Users/sebastian/projects/apps/herdr/crates/ghostty-vt` and `/Users/sebastian/projects/apps/herdr/vendor/portable-pty` into `/Users/sebastian/Projects/apps/super-lope/`, preserving `Apache-2.0` license headers (`NFR12`).
   - Scaffold workspace crates: `lope-core`, `lope-detect`, `lope-runtime-pty`, `lope-providers`, `lope-memory`, `lope-engine`, `lope-flow`, `lope-tui`, `lope-cli`.
2. **`lope-core` Models, Config & Verdict Parser**:
   - Port `VerdictStatus` (`Pass`, `NeedsFix`, `Fail`, `InfraError`, `Blocked`), `ValidatorResult`, `PhaseVerdict`, `Phase`, `SprintDoc`, `ExecutionReport`, `OutcomeClass`, and `LopeConfig` (`~/.lope/config.json` reader/writer with atomic `.tmp` rename).
   - Port `parse_verdict_block` (JSON-first, YAML/regex fallback) and `verdict_repair` prompt builder from `lope/validators.py` and `lope/verdict_repair.py`.
   - Port `redact_text` from `lope/redaction.py` covering API tokens (`sk-`, `ghp_`, `AKIA`, `AIza`, `xoxb-`), PEM blocks (`-----BEGIN`), Bearer headers, and configured secret env vars.
3. **`lope-cli` Complete 27-Subcommand Grammar (`FR1`, `FR2`, `FR5`)**:
   - Implement `clap` parsers for all 23 legacy verbs (`negotiate`, `execute`, `implement`, `audit`, `ask`, `review`, `vote`, `compare`, `pipe`, `team`, `deliberate`, `memory`, `gate`, `check`, `status`, `jobs`, `configure`, `install`, `update`, `upgrade`, `version`, `docs`, `flow`) plus the 4 new verbs (`daemon`, `pool`, `attach`, `detect`).
   - Ensure `lope version`, `lope --version`, and `lope -V` all print `lope 2.0.0 (super-lope; compat 0.17.3)` and exit `0` in `< 10ms`.
   - Ensure `lope review` accepts both boolean `--consensus` (defaulting to `quorum`) and `--consensus <mode>`.

## Acceptance Gate & Evidence (`docs/evidence/s0-workspace.txt`)

- `cargo test --workspace` passes `A1` (CLI parsing & `< 10ms` startup) and `A2` (`config.json` round-trip, redaction, and `---VERDICT---` parser parity).
- Write test summary and startup timing into `docs/evidence/s0-workspace.txt`.

---

[Sprints Index](README.md) · Next: [S1 — PTY Supervisor & Detector](S1-pty-supervisor-and-detector.md)
