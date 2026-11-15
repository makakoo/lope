# Handoff — Super Lope (2026-11-14)

**Status:** S0–S6 verified complete. S7 and S8 code landed at commit `0cc251b` (`S8: install, docs, release binary cutover, benchmarks, and evidence`), **but cutover closeout is NOT complete.** S7 parity defects, S8 doc/install hygiene issues, cross-repo patches, and live `A17` / `M1–M8` acceptance verification remain open and are tracked in [S8B — Remediation & Cutover Closeout](S8B-remediation-and-cutover-closeout.md).

**Do not resume Rust/external coding until the S8B plan and updated specs are reviewed/approved.**

## Live Host & Workspace State

| Location | State (`2026-11-14`) | Notes |
|---|---|---|
| `/Users/sebastian/projects/apps/super-lope` | Clean at `0cc251b` | Rust release + debug builds pass unit/partial parity tests, but `s7-sprint-flow.json` and `s0-delegation.json` were relaxed/trimmed during S7 |
| `/usr/local/bin/lope` | Rust `2.0.0 (super-lope; compat 0.17.3)` | Live default binary (`~/bin/lope` retargeted; `~/.zshrc:178` alias removed) |
| `/usr/local/bin/lope-py` | Python `0.17.3` shim | Intact rollback target (`LOPE_FORCE_PYTHON=1` also routes to Python) |
| `/Users/sebastian/.bashrc` | **Patch `PH-1` incomplete** | Line 33 still sets `alias lope='PYTHONPATH=/Users/sebastian/.lope python3 -m lope'` |
| `/Users/sebastian/.lope` | Clean baseline (`9cffa55`, tag `super-lope-baseline-0.17.3`) + local uncommitted mods | `PL-2` (`~/.lope/.gitignore` `super-lope/`) and `PL-3` (`skills/` & `commands/` rewrite to `lope`) still unapplied |
| `/Users/sebastian/projects/apps/makakoo-jungle` | Clean at `8a541055` | `PG-1..PG-8` unapplied (`bin/glimmer`, `lib/companion/server.js`, `council-runner.ts`, `plan-build.ts`, `jobs.js`, etc.) |
| `/Users/sebastian/projects/apps/makakoo-os` | Clean at `2e1bfe7` | `PM-2` unapplied in `makakoo/src/commands/setup/lope.rs`; `PM-1` deferred per DEC-8 |
| `/Users/sebastian/MAKAKOO` | Active Makakoo workspace | `PM-3` unapplied in `AGENTS.md:100` (`PYTHONPATH=~/.lope python3 -m lope configure`) |

## Verified Open Gaps (Summary)

1. **S7 Engine & CLI Parity (`FR32–FR36`, `A11`, `A15`, `A16`)**:
   - **Critical (`FR4`):** `crates/lope-cli/src/native/scope.rs:104` sets `CallContext.implementation = false`, and neither `crates/lope-engine/src/implement.rs` nor `crates/lope-cli/src/native/execute.rs` sets `implementation = true` for writer calls. As a result, `crates/lope-runtime/src/call.rs:271` never increments `LOPE_IMPLEMENTATION_DEPTH` and `crates/lope-providers/src/builtin.rs:421` runs Codex writers in `read-only` sandbox mode instead of `workspace-write`.
   - **Untrusted gate exit code (`FR34`, `FR3`):** `gate.rs` (`run_check`, `run_gate_save`, `run_gate_check`), `execute.rs:61`, and `implement.rs:84` return exit `2` instead of Python's `3` (`cli.py:1147, 1177, 1247`). Also verify `check --json` / `check` text parity (`check-json-trust` was dropped from `s7-sprint-flow.json`).
   - **`execute` parity (`FR33`):** 0-phase exits `1` instead of `2`; `--manual` ignores stdin EOF / non-TTY instead of exiting `3` (`cli.py:2724-2728`).
   - **`implement` parity (`FR33`, `FR35`, `A11 G8`):** `--interactive` (`_interactive`) is ignored; 0-phase exits `1` instead of `2`; `render_dry_run` format diverges (`implement-dry-run` was dropped from `s7-sprint-flow.json`).
   - **`negotiate` parity (`FR32`, `A11 G7`):** preflight warnings omit Python byte/line/timeout threshold env knobs; progress callback (`log_ev`) is a no-op; escalated runs fail to write `<out>`, `<out>.feedback.md`, and `<out>.rounds/` sidecars; `slug_from_goal` delegates to `slug_from_title` (differs from `[:60]` rule); stage uses `"negotiate_propose"` instead of `"negotiate_draft"`. `tools/parity/golden_run.py` + `tools/parity/corpus/s0-delegation.json:256` added `"ignore": ["stderr", "snapshot"]` to pass `glimmer-negotiate-rounds`.
   - **`SelfHealer` (`FR35`):** `crates/lope-engine/src/healer.rs` is compiled but has zero callers in `crates/`.
   - **`flow` parity (`FR36`, `A16`):** `validate.rs:135` and `native/flow.rs:91` fall back to `50` instead of `max(50, 8 * nodes)`; `FlowRunner` lacks `LOPE_FLOW_WORKERS` bounded wave concurrency.
2. **S8 Binary Hygiene & Docs (`FR37`, `FR39`)**:
   - `crates/lope-cli/src/native/install.rs:38` and `tests/s8_cutover_parity.rs:28` contain hardcoded `/Users/sebastian/.lope`. Check `--with-headroom` passthrough against `~/.lope/install`.
   - `crates/lope-cli/assets/reference.md` is still Python `0.17.3` documentation (unresolved `FR39`), and `crates/lope-cli/src/native/docs.rs:9-23` lets `$HOME/.lope/docs/reference.md` shadow the embedded reference.
3. **Cross-Repo Patches & S8 Evidence (`A17`, `M1–M8`)**:
   - `PH-1` (`~/.bashrc:33`), `PL-2`/`PL-3`, `PG-1..PG-8`, `PM-2`, `PM-3` remain unapplied.
   - `A17` (Glimmer test suite vs `glimmer-baseline-2026-11-07.txt`, `glimmer doctor`, `makakoo setup lope`, `gym_lope_gate` dry run, `lope-py version`) and `M1–M8` (live/GUI/Plan->Build/rollback/20-run p50+p95+router+5-validator RSS+3-validator review wall-clock benchmarks) have not been executed.

## Next Step

Execute [S8B — Remediation & Cutover Closeout](S8B-remediation-and-cutover-closeout.md) starting at **Phase S8B.A**.
