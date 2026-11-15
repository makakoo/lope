# Handoff — Lope v2 (2026-11-15)

**Status:** S0–S8 and **S8B** (`docs/sprints/S8B-remediation-and-cutover-closeout.md`) complete on `makakoo/lope` (`v2`). `/usr/local/bin/lope` (`lope 2.0.0 (compat 0.17.3)`) is live as the default CLI; `/usr/local/bin/lope-py` (`sprint runner v0.17.3`) remains installed for instant rollback (`LOPE_FORCE_PYTHON=1` or copying `/usr/local/bin/lope-py`).

## Live Host & Workspace State

| Location | State (`2026-11-15`) | Notes |
|---|---|---|
| `/Users/sebastian/projects/lope` | `v2` (S8B closeout) | All workspace crates, A1 argparse↔clap diff (`48` commands, `347` arguments), and strict `golden_run.py` (`s0`–`s7`, zero `"ignore"` hacks) pass. Evidence in `docs/evidence/s7-sprint-flow.txt` and `docs/evidence/s8-cutover.txt`. |
| `/usr/local/bin/lope` | Rust `2.0.0 (compat 0.17.3)` | Live default binary (`~/bin/lope` wraps `/usr/local/bin/lope`; `~/.bashrc:33` legacy alias commented out). |
| `/usr/local/bin/lope-py` | Python `0.17.3` shim | Intact rollback target (`LOPE_FORCE_PYTHON=1` also routes to Python). |
| `/Users/sebastian/.lope` | `main` (`9cffa55`) + `PL-2`/`PL-3` + WIP | `PL-2` (`v2/` in `.gitignore`) and `PL-3` (`skills/` & `commands/` updated for native `lope`, preserving `skills/remote-attachments` symlink and pre-existing WIP Python edits); all 8 agent hosts refreshed via `lope install --host all`. |
| `/Users/sebastian/projects/apps/makakoo-jungle` | `main` + `PG-1..PG-8` (12 allowlisted files) | `glimmer doctor` (`Lope: ✓ 2.0.0`), companion `team.get`/`jobs.js`/`edges.ts`, 60-char slug rule, zsh/bash `pipefail` TSX capture, and regression tests landed; Glimmer suites `tests/companion` (12/12), `s2` (8/8), `s3` (10/10), `s5` (10/10), `s6` (7/7), `s7` (9/9), `s8` (10/10) pass (`tests/s4` has 1 unrelated failure calling `switchAILocal *:18080` ollama `auth_unavailable`). |
| `/Users/sebastian/projects/apps/makakoo-os` | `main` + `PM-2` | `makakoo/src/commands/setup/lope.rs` updated (`lope version` probe, `makakoo/lope` origins accepted, no `PYTHONPATH` alias hint); `PM-1` (`gym_lope_gate`) intentionally stays deferred per `DEC-8`. |
| `/Users/sebastian/MAKAKOO` | `AGENTS.md` (`PM-3`) + Brain logged | `AGENTS.md:100` updated to `lope configure`; Brain journal synced. |

## Open Items / Gated Next Phase

- **Interactive Electron UI & nightly cron observation (`M2`, `M4`, `M5`):** Core logic and headless/companion CLI paths are verified in unit tests and live headless companion execution (`M3` + cancel teardown); visual Electron click-through in Glimmer.app and the next unattended nightly `gym_lope_gate` cron window occur on next normal desktop/cron use.
- **External repo commits:** Modified patch sets in `makakoo-jungle`, `makakoo-os`, `~/.lope`, and `~/MAKAKOO` are staged/tested locally on disk ready for their own repository commits if desired.
- **S9 / OQ-4 evaluation (`S9-pty-pool-and-daemon.md`):** Unblocked now that S8/S8B evidence is complete; gated behind explicit `OQ-4` approval based on `FR12` interactive TTY evidence.
