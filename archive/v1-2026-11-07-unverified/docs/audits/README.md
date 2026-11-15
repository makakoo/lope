# Super Lope System Audits

[Root Index](../../README.md) · **Audits** · [Specs](../specs/README.md) · [Sprints](../sprints/README.md)

This directory records the verified ground truth of the live codebases on the dogfood Intel Mac (`2026-11-07`) that **Super Lope** fuses and serves:
1. **Python Lope (`0.17.3`)** at `/Users/sebastian/.lope`
2. **Rust Herdr (`0.9.5`)** at `/Users/sebastian/projects/apps/herdr`
3. **Makakoo OS (`0.4.0`)** at `/Users/sebastian/projects/apps/makakoo-os` & `/Users/sebastian/MAKAKOO`
4. **Glimmer (`makakoo-jungle`)** at `/Users/sebastian/Projects/apps/makakoo-jungle`

Every architectural decision in [`docs/specs/`](../specs/README.md) and every sprint gate in [`docs/sprints/`](../sprints/README.md) derives directly from these verified code facts.

---

## Audit Files

| # | Audit Document | Scope | Primary Backlinks |
|---|---|---|---|
| 1 | [01 — Python Lope Audit (`0.17.3`)](01-lope-python-audit.md) | Complete module inventory (`61` `.py` files, `27,322` LOC), 23 CLI subcommands, `supervisor.py` & `processes.py` root-cause timeout/reaping bugs, validator adapters, deliberation, flow runner, SQLite memory, and state manifests | [Architecture](../specs/01-product-and-architecture.md), [Interfaces](../specs/04-interfaces-and-protocol.md), [S0](../sprints/S0-workspace-and-compat-cli.md), [S1](../sprints/S1-pty-supervisor-and-detector.md), [S3](../sprints/S3-validators-and-http-engine.md)–[S7](../sprints/S7-sprints-and-flow-graphs.md) |
| 2 | [02 — Rust Herdr Audit (`0.9.5`)](02-herdr-rust-audit.md) | Cargo workspace (`herdr` + `crates/ghostty-vt`), `291,081` LOC in `src/`, PTY actor (`src/pty/actor.rs`), screen detector (`src/detect/`), 22 agent `.toml` manifests, headless daemon (`src/server/headless.rs`), `wait_for_output` (`src/api/wait.rs`), and licensing (`Apache-2.0`) | [Architecture](../specs/01-product-and-architecture.md), [Requirements](../specs/03-requirements-and-acceptance.md), [S0](../sprints/S0-workspace-and-compat-cli.md), [S1](../sprints/S1-pty-supervisor-and-detector.md), [S2](../sprints/S2-warm-pool-and-daemon.md), [S6](../sprints/S6-council-deliberation.md) |
| 3 | [03 — Makakoo OS & Glimmer Seams Audit](03-makakoo-and-glimmer-seams-audit.md) | Makakoo OS (`0.4.0`) Rust workspace crates, `makakoo search` / `sync` contracts, Glimmer (`makakoo-jungle` @ `8a541055`) CLI/daemon invocation seams, `glimmer doctor` version regex, and the 12 installed Lope skills | [Use Cases](../specs/02-use-cases-and-scenarios.md), [Interfaces](../specs/04-interfaces-and-protocol.md), [S0](../sprints/S0-workspace-and-compat-cli.md), [S5](../sprints/S5-memory-and-makakoo-bridge.md), [S8](../sprints/S8-glimmer-cutover-and-benchmarks.md) |

---

## Live Pin Table (Verified 2026-11-07)

Before starting [Sprint S0](../sprints/S0-workspace-and-compat-cli.md), re-verify this table on the dogfood Mac and write it to `docs/evidence/pins.txt`.

| Component | Verified State (`2026-11-07`) | Contract Rule |
|---|---|---|
| **Hardware & OS** | macOS (`x86_64`, Intel Core `i5-8257U`, `16 GB` RAM, Intel Iris Plus Graphics 645, Homebrew prefix `/usr/local`). | No local vLLM/MLX. Memory and process count must stay strictly bounded (`< 15 MB` idle RSS, max `5` concurrent local PTYs by default; [`NFR2`](../specs/03-requirements-and-acceptance.md#non-functional-requirements)). |
| **Python Lope (`~/.lope`)** | Branch `main`, commit `9cffa55` (`0.17.3` dirty dev tree). `61` `.py` files (`53` in `lope/` + `8` in `lope/flow/`), `27,322` total lines. | Keep `~/.lope` intact as fallback (`lope-py`) during development until [Sprint S8](../sprints/S8-glimmer-cutover-and-benchmarks.md) cutover passes all parity gates ([`FR4`](../specs/03-requirements-and-acceptance.md#functional-requirements)). |
| **Rust Herdr (`~/projects/apps/herdr`)** | Branch `master`, commit `ee5bfb50`, package version `0.9.5`, `Apache-2.0` license. `291,081` lines of `.rs` in `src/`, `22` `.toml` manifests in `src/detect/manifests/` (`2,186` lines), plus `crates/ghostty-vt` and `vendor/portable-pty`. | Extract/reuse `crates/ghostty-vt`, `vendor/portable-pty`, `src/pty/`, and `src/detect/` directly in the Super Lope Cargo workspace while preserving `Apache-2.0` attribution ([`NFR12`](../specs/03-requirements-and-acceptance.md#non-functional-requirements)). |
| **Makakoo OS (`~/projects/apps/makakoo-os`)** | Version `0.4.0` (`/Users/sebastian/.local/bin/makakoo` & `/usr/local/bin/makakoo`), persona `Harvey`, `MAKAKOO_HOME=/Users/sebastian/MAKAKOO`, `MIT` license. Rust edition `2021` (`tokio 1.40`, `rusqlite 0.32`, `clap 4`, `reqwest 0.12`). | Align Super Lope workspace dependencies (`tokio`, `rusqlite`, `serde`, `clap`, `reqwest`, `tracing`, `blake3`, `sha2`) with `makakoo-os/Cargo.toml` ([`NFR1`](../specs/03-requirements-and-acceptance.md#non-functional-requirements)). |
| **Glimmer (`~/Projects/apps/makakoo-jungle`)** | Branch `main`, commit `8a541055`, CLI `/usr/local/bin/glimmer`. `glimmer doctor` verifies `Makakoo OS: 0.4.0`, `Brain FTS: ready`, `Lope: 0.17.3`, `Glimmer Shim: installed`. | Super Lope's `lope version` output, `lope status` output, `lope review` flags, `lope negotiate --domain`, `lope implement`, and `lope jobs prune --stale` must pass `glimmer doctor` and all Glimmer `A1`–`A15` tests unmodified ([`FR1`–`FR4`](../specs/03-requirements-and-acceptance.md#functional-requirements)). |

---

## Verified Root-Cause Defect Matrix (Why Python Lope Fails & How Super Lope Fixes It)

| # | Defect Observed in Production | Exact Root Cause in Python Lope (`~/.lope`) | Super Lope Rust Fix (Herdr + Lope Fusion) |
|---|---|---|---|
| **D1** | Valid model completions are thrown away as `provider completion unverified` / `cleanup_failed:descendants_live` (`20/21` abandoned manifests in audit). | `lope/supervisor.py:102-118`: `_process_group_count(pgid)` has `if sys.platform == "darwin": return 1` whenever `os.killpg(pgid, 0)` succeeds. When a CLI finishes and exits `0` (`stdout_complete=True`) but a child MCP/helper process takes `> 0.7s` to exit, `supervisor.py:489-491` overrides `outcome = "cleanup_failed"` and `processes.py:314` raises `ProcessCompletionError`, discarding the answer. | **Leader-Completion Authority (`FR9`)**: In `lope-runtime-pty`, when the direct child PID exits `0` and the terminal/output stream contains a complete response, the call is immediately marked `OutcomeClass::Ok`. Any lingering process-group descendants are reaped asynchronously in a background task without failing the call. |
| **D2** | Validator calls hang silently for `120s`–`900s` before timing out when invoked on new worktrees or after CLI updates. | `lope/supervisor.py:355-368`: Providers are spawned with `stdin=PIPE, stdout=PIPE, stderr=PIPE` (no PTY). When `codex`, `claude`, or `agy` displays a Folder Trust prompt, Startup Update prompt, or Tool Permission prompt, it waits on TTY input while Python Lope sits blind until `deadline`. | **Real-Time PTY Screen Detection (`FR10`–`FR12`)**: `lope-runtime-pty` runs CLI validators in `portable-pty` + `ghostty-vt` and evaluates `lope-detect` (`22` Herdr `.toml` manifests) on every screen diff. When `AgentState::Blocked` fires (`visible_blocker == true`), the Blocker Policy Engine either auto-remediates safe prompts (`trust_directory`, `startup_update`) or aborts in `< 250ms` with the exact blocker rule ID and screen tail. |
| **D3** | High CPU load, sluggish startup (`1–3s` before any network call), and fan spin on the Intel Mac during parallel ensemble runs. | `lope/processes.py:231-270`: Every validator call spawns a new `python3 -m lope.supervisor` process, which spawns a cold Node.js/Python CLI process, while the parent thread busy-polls `supervisor.poll()` every `20ms` (`time.sleep(0.02)`) and rewrites JSON files to disk. | **Event-Driven `tokio` + Warm Agent Pool (`FR13`–`FR15`)**: Zero Python processes, zero `20ms` polling loops (`kqueue` async event notifications via `tokio`), direct in-process `reqwest` pool for HTTP providers, and warm reusable CLI panes in `loped` for multi-round workflows. |
| **D4** | `lope --version` fails with `error: unrecognized arguments: --version` while `lope version` works (or prints a noisy multi-line ASCII banner). | `lope/cli.py`: Top-level `argparse` parser omits `--version` / `-V` flag and only registers the `version` subcommand, which prints a multi-line logo unless parsed carefully by `glimmer doctor`. | **Dual Version Contract (`FR2`)**: Support `lope version`, `lope --version`, and `lope -V`. Keep `lope version` compatible with `glimmer doctor`'s regex (`>= 0.17.3` / `2.0.0`) while supporting `--quiet` / `--json`. |
| **D5** | Child background helpers holding `stdout`/`stderr` FDs open cause `output_drain_timeout` after the CLI exits. | `lope/supervisor.py:69-100, 484, 492`: When a CLI spawns a background daemon that inherits `stdout`/`stderr` pipe FDs, `streams_open` remains non-empty after `0.5s` drain, causing `supervisor.py` to fail with `output_drain_timeout`. | **PTY Master EOF & Leader Exit Decoupling (`FR9`)**: `lope-runtime-pty` tracks leader process exit (`waitpid`) independently of slave FD inheritance and drains buffered PTY bytes immediately without failing on inherited FDs. |

---

[Root Index](../../README.md) · Next: [01 — Python Lope Audit](01-lope-python-audit.md)
