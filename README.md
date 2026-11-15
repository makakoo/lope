# Lope v2 — Rust CLI & Validator Ensemble (`makakoo/lope`)

**Multi-CLI validator ensemble for AI work.** One AI CLI drafts; others validate. Works for multi-phase sprints (`negotiate` → `implement`/`execute` → `audit`), autonomous graph workflows (`flow`), single-shot multi-model tasks (`ask`, `review`, `vote`, `compare`, `pipe`), council deliberation (`deliberate`), objective evidence gates (`gate`, `check`), persistent judgment (`memory`), Makakoo Brain integration, and roster management (`team`).

**Repository & branches:**
- **`makakoo/lope` (`v2` branch):** Rust Lope 2.0 (`lope 2.0.0 (compat 0.17.3)`), built to be behaviourally compatible with Python Lope `0.17.3` (`traylinx/lope`, kept untouched on `main` as legacy v0.x).
- **Build entry point:** Always build via `scripts/cargo` (respecting `rust-toolchain.toml`, working around Homebrew Rust preceding rustup on macOS PATH).

```sh
scripts/cargo build --release            # target/release/lope
scripts/cargo test --workspace           # unit, router, engine, and parity tests
tools/parity/dump-python && tools/parity/dump-rust && tools/parity/diff   # A1 grammar check
LOPE_ROUTER_TRACE=1 target/release/lope review notes.md --consensus quorum
```

Router controls: `LOPE_FORCE_PYTHON=1` (delegate everything), `LOPE_FORCE_NATIVE=1` (uncovered → exit 2), `LOPE_PYTHON` (interpreter), `LOPE_ROUTER_TRACE=1`.

## Documentation & Workspace Map

```text
README.md                         this file
SPRINT-LOPE-V2.md                 master v2 sprint plan
docs/
├── audits/                       verified baseline audits & errata (2026-11-07)
├── reference/                    machine-captured 0.17.3 CLI/SQLite/env baselines
├── specs/                        requirements, architecture, and interface contracts
├── sprints/                      S0–S9 implementation specs & S8B closeout plan
└── evidence/                     per-sprint parity and benchmark records
Cargo.toml, rust-toolchain.toml   Rust workspace (pinned toolchain 1.96.1)
crates/                           lope-core, lope-compat, lope-state, lope-runtime,
                                  lope-providers, lope-memory, lope-makakoo,
                                  lope-engine, lope-flow, lope-cli
tools/parity/                     argparse ↔ clap dump/diff (A1), golden-run harness (A2)
scripts/cargo                     pinned-toolchain wrapper
install, skills/, commands/       host integrations (Claude, Codex, Gemini, OpenCode, Cursor, Vibe, Qwen, pi)
```

## Non-negotiables

- Treat [docs/reference/](docs/reference/) and `tools/parity/` as the compatibility baseline.
- Keep `~/.lope` state (`config.json` v1, `memory.db` `user_version = 1`, `~/.lope/runs/`) readable by Python Lope while rollback (`LOPE_FORCE_PYTHON=1` / `/usr/local/bin/lope-py`) and Makakoo's `gym_lope_gate` (DEC-8 / PM-1) remain active.
- Cross-repo changes in Glimmer (`makakoo-jungle`), Makakoo OS (`makakoo-os` / `~/MAKAKOO`), and `~/.lope` are tracked in [Spec 04 §8](docs/specs/04-interfaces-and-protocol.md#8-cross-repo-patch-sets).
