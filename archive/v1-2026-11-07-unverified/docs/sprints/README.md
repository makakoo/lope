# Super Lope — Sprint Roadmap & Execution Plan (`S0`–`S8`)

[Root Index](../../README.md) · [Master Sprint (`SPRINT-SUPER-LOPE-V1.md`)](../../SPRINT-SUPER-LOPE-V1.md) · [Audits](../audits/README.md) · [Specs](../specs/README.md) · **Sprints**

This directory owns the serial implementation roadmap, build progression, traceability matrix, and individual sprint specifications (`S0` through `S8`) for **Super Lope** (`lope` v2.0 in Rust), alongside the consolidated master sprint file [`SPRINT-SUPER-LOPE-V1.md`](../../SPRINT-SUPER-LOPE-V1.md).

---

## Sprint Index

| Sprint | Document | Primary Focus | Evidence Output |
|---|---|---|---|
| **S0** | [`S0-workspace-and-compat-cli.md`](S0-workspace-and-compat-cli.md) | 10-crate Cargo workspace, `ghostty-vt` & `portable-pty` extraction from Herdr, `lope-core` models/config/verdict parser, 27-verb `clap` CLI grammar | `docs/evidence/pins.txt`, `docs/evidence/s0-workspace.txt` |
| **S1** | [`S1-pty-supervisor-and-detector.md`](S1-pty-supervisor-and-detector.md) | `lope-runtime-pty` async PTY actor, Leader-Completion Darwin reaper (`Fixes D1/D5`), `lope-detect` 22-agent `.toml` manifests, Blocker Policy Engine (`Fixes D2`) | `docs/evidence/s1-pty-detector.txt` |
| **S2** | [`S2-warm-pool-and-daemon.md`](S2-warm-pool-and-daemon.md) | `loped` daemon (`lope.socket.v1`), `LOCAL_PEERCRED` auth, Tier 3 Warm Agent Pool (`lope pool`), run registry & `lope jobs prune --stale` | `docs/evidence/s2-daemon-pool.txt` |
| **S3** | [`S3-validators-and-http-engine.md`](S3-validators-and-http-engine.md) | `lope-providers`: Tier 1 native `reqwest` HTTP engine, 15 built-in CLI adapters, `--from-curl` parser, `lope team` CLI, budgets/leases/retries | `docs/evidence/s3-validators.txt` |
| **S4** | [`S4-single-shot-and-consensus-review.md`](S4-single-shot-and-consensus-review.md) | Single-shot verbs (`ask`, `vote`, `compare`, `pipe`), `lope review --consensus`, finding deduplication/scoring, 7 Role Lenses (`--focus`), `--output`, SARIF | `docs/evidence/s4-review.txt` |
| **S5** | [`S5-memory-and-makakoo-bridge.md`](S5-memory-and-makakoo-bridge.md) | `lope-memory` (`rusqlite` WAL + FTS5 `~/.lope/memory.db`), `lope memory` CLI, Makakoo Brain bridge (`--brain-context`, `--brain-log` + `makakoo sync`) | `docs/evidence/s5-memory-brain.txt` |
| **S6** | [`S6-council-deliberation.md`](S6-council-deliberation.md) | 7-Stage Council Deliberation (`lope deliberate` across 6 templates) + `lope attach` live Ratatui split-pane TUI viewer (`lope-tui`) | `docs/evidence/s6-deliberate-tui.txt` |
| **S7** | [`S7-sprints-and-flow-graphs.md`](S7-sprints-and-flow-graphs.md) | Sprint engine (`negotiate --domain`, `execute`, `implement`, `audit`, `gate`, `check`) + Declarative Graphviz DOT flow runner (`lope flow`) | `docs/evidence/s7-sprints-flow.txt` |
| **S8** | [`S8-glimmer-cutover-and-benchmarks.md`](S8-glimmer-cutover-and-benchmarks.md) | `/usr/local/bin/lope` cutover (`lope-py` fallback preserved), `glimmer doctor` & watched Council end-to-end verification, Intel Mac latency/RSS benchmark | `docs/evidence/s8-cutover-bench.txt` |

---

## Execution Rules

1. **Strictly Serial (`S0` → `S8`)**: Sprints execute one at a time. Sprint `S(n)` starts only when the evidence file(s) for `S(n-1)` exist on disk under `docs/evidence/`.
2. **Offline Deterministic Tests First**: Every sprint's unit and integration tests (`A1`–`A16`) must pass offline using deterministic mock PTY scripts and local `axum` mock HTTP servers before live dogfood tests (`M1`–`M8`) run.
3. **Preserve Python Fallback Until `S8`**: Until `S8` completes, `/usr/local/bin/lope` remains untouched while the Rust binary builds and tests in `/Users/sebastian/Projects/apps/super-lope/target/release/lope`. In `S8`, `/usr/local/bin/lope-py` is linked to the legacy Python runner and `/usr/local/bin/lope` is atomically switched to the compiled Rust binary.
4. **Evidence Hygiene (`NFR13`)**: Evidence files under `docs/evidence/` record test outputs, timings, and benchmark tables. They never contain API keys or raw `~/.lope/config.json` secrets.

---

## Traceability Matrix

| Sprint | Functional Requirements | Non-Functional Requirements | Automated Tests | Scenarios & Manual Checks |
|---|---|---|---|---|
| **S0** | `FR1`–`FR3`, `FR5`, `FR23`, `FR24` | `NFR1`, `NFR5`, `NFR8`, `NFR12` | `A1`, `A2` | `SC1` |
| **S1** | `FR9`–`FR12`, `FR14`, `FR39`, `FR40` | `NFR4`, `NFR7`, `NFR9`, `NFR14` | `A3`, `A4`, `A5` | `SC2`, `SC3`, `SC4`, `M2` |
| **S2** | `FR6`, `FR15`, `FR38` | `NFR2`, `NFR3`, `NFR6`, `NFR11` | `A6` | `SC5`, `SC14`, `M3` |
| **S3** | `FR7`, `FR8`, `FR13`, `FR16` | `NFR4`, `NFR5`, `NFR11` | `A7`, `A8` | `SC6` |
| **S4** | `FR17`–`FR22` | `NFR5`, `NFR8`, `NFR11` | `A9`, `A10` | `SC7`, `M1` |
| **S5** | `FR25`–`FR27` | `NFR5`, `NFR10` | `A11`, `A12` | `SC8`, `M7` |
| **S6** | `FR28`, `FR34`, `FR35` | `NFR2`, `NFR8` | `A13` | `SC9`, `SC10`, `M4` |
| **S7** | `FR29`–`FR33` | `NFR4`, `NFR8` | `A14`, `A15` | `SC11`, `SC12`, `SC13` |
| **S8** | `FR4`, `FR36`, `FR37` | `NFR1`, `NFR2`, `NFR13`, `NFR15` | `A16` | `SC1`, `SC7`, `SC15`, `M5`, `M6`, `M8` |

---

[Root Index](../../README.md) · Next: [S0 — Workspace & Compat CLI](S0-workspace-and-compat-cli.md)
