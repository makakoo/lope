# Spec 02 — Use cases

[Specs](README.md) · Previous: [01 — Architecture](01-product-and-architecture.md) · Next: [03 — Requirements](03-requirements-and-acceptance.md)

Every use case is tied to a real caller from [Audit 03](../audits/03-makakoo-and-glimmer-seams-audit.md) or a real Python Lope feature from [Audit 01](../audits/01-lope-python-audit.md). Revision 1's separate scenario list (SC1–SC15) has been folded into the acceptance tests A1–A18 and M1–M8, so each behaviour has one test ID.

| ID | Use case | Caller / context | Postcondition | Tests |
|---|---|---|---|---|
| **UC1** | Version and health probe | Glimmer doctor (G1, C4), companion health (G2, C3), `makakoo setup lope` (M2) | `lope version` exits 0 within 5 s under the launchd env. The first `N.N` on stdout is the version. `--version` and `-V` also work. | A1, A11 |
| **UC2** | Glimmer Council review (watched tab) | G4, C2 | `review <notes> --consensus quorum --format markdown [--focus <prose>] [--brain-context q] [--remember] [--brain-log]` is accepted. Stdout is the report. Progress goes to stderr. Exit code follows Python semantics. | A10, A11, M2 |
| **UC3** | Headless review | G5, C3 | `review <file> --consensus --format markdown` writes the report to stdout. SIGTERM to the Lope PID tears down every child process group. | A10, A11, M3 |
| **UC4** | Plan (negotiate) | G7, C2; CLI users | `negotiate <goal> --domain d --rounds N` (alias) or `--max-rounds N` writes `SPRINT-<SLUG>.md` using Python's slug rule and the `.rounds` sidecar. | A15, M4 |
| **UC5** | Build (implement) | G8, C2 | `implement <abs-sprint-path>` with no flags prompts for a roster on a TTY, as Python does. Real flags `--agents/--escalate-to/…` work. Writers get `LOPE_IMPLEMENTATION_DEPTH = 1`. | A15, M4 |
| **UC6** | Single-shot fan-out | skills (`lope-ask`, `lope-vote`, `lope-compare`, `lope-pipe`), users | `ask/vote/compare/pipe` match Python output and isolation semantics, including `--require-all`, `--synth`, `--anonymous` and `--json`. | A9 |
| **UC7** | Team management | `lope-team` skill, users | `team list/enable/disable/add/remove/test/health`, including `--from-curl`, edits `config.json` in Python format. Python can read the result. | A3, A8 |
| **UC8** | Council deliberation | `lope-deliberate` skill | `deliberate <template> <scenario|-> [--depth] [--out DIR] …` produces the same artifact set as Python. | A14 |
| **UC9** | Finding memory | `lope-memory` skill, `review --remember` | `memory stats/search/file/hotspots/forget/gates` run on the unchanged schema. Rust and Python can both read and write. | A12 |
| **UC10** | Makakoo Brain bridge | Glimmer G4 flags, `lope-*` skills | `--brain-context` injects a cleaned, bounded block once. `--brain-log` appends a locked, local-date bullet and runs `makakoo sync --file`. | A13, M6 |
| **UC11** | Makakoo-hosted validators | `makakoo adapter call` adapters (`pi`, `switchailocal`, `tytus-cli`) | Discovered and invoked exactly as Python does. | A8 |
| **UC12** | Sprint execution, gates, audit | `lope-execute`/`lope-audit` skills, CI | `execute`, `gate save/check`, `check` and `audit` behave as Python, including trust and exit 3, and the audit journal. | A15 |
| **UC13** | Flow graphs | `lope-flow` skill | `flow run/validate/lint/render/init/list` follow Python's node types, interpolation and caps. | A16 |
| **UC14** | Run registry hygiene | Glimmer G3, operators | `status`, `jobs list/reap/prune/kill` on `~/.lope/runs/`. Dead runs are pruned; live runs are never touched. | A4 |
| **UC15** | Nightly Makakoo gym gate | SANCHO `gym_lope_gate` (M1, C5) | Keeps passing: it still imports `~/.lope` Python until PM-1 migrates it to a CLI contract. | M5 |
| **UC16** | Rollback | operator | Pointing `/usr/local/bin/lope` back at Python (or setting `LOPE_FORCE_PYTHON=1`) works against all state written by Rust. | A2, A3, A4, A12, M7 |
| **UC17** | Warm pool (post-cutover, gated) | users, after OQ-4 | Opt-in interactive providers reuse warm PTY panes in `loped`. Blockers are handled by Lope-authored rules. | A18 (S9) |

## Operator journeys

1. **Developer dogfooding during S1–S7.** Run `target/release/lope` directly, or set `LOPE_BIN` for skills that honour it. `LOPE_FORCE_PYTHON=1` gives an instant A/B comparison. `/usr/local/bin/lope` stays Python until S8.
2. **Cutover (S8).**
   - Install the Rust binary at `/usr/local/bin/lope` and keep `/usr/local/bin/lope-py`.
   - Apply PG-1 so that `glimmer install` no longer overwrites it.
   - Apply PH-1 and PH-2 (remove the alias, retarget `~/bin/lope`).
   - Run `glimmer doctor`, the Glimmer suite (with documented pre-existing failures), the M-checks, and the gym gate.
3. **Rollback.** `ln -sf lope-py /usr/local/bin/lope`, or set `LOPE_FORCE_PYTHON=1` globally. No state migration is needed (NFR6).
