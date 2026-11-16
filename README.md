# Lope v2 — Rust CLI & Validator Ensemble (`makakoo/lope`)

**Multi-CLI validator ensemble for AI work.** One AI CLI drafts; others validate. Works for multi-phase sprints (`negotiate` → `implement`/`execute` → `audit`), autonomous graph workflows (`flow`), single-shot multi-model tasks (`ask`, `review`, `vote`, `compare`, `pipe`), council deliberation (`deliberate`), objective evidence gates (`gate`, `check`), persistent judgment (`memory`), Makakoo Brain integration, and roster management (`team`).

**Repository & versions:**
- **`makakoo/lope` (`v2` branch):** Rust Lope 2.0 (`lope 2.0.1 (compat 0.17.3)`), targeting POSIX (`macos`, `linux`).
- **Python compatibility & rollback (`0.17.3`):** Frozen compat baseline (`traylinx/lope`, local `lope-py` / `lope-agent` wheel) preserved for `LOPE_FORCE_PYTHON=1` rollback and schema/golden parity testing.

## Installing Prebuilt Releases (macOS & Linux)

Download the release tarball for your architecture (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`) along with its `.sha256` sidecar (or `SHA256SUMS`), then verify checksums + Sigstore provenance before installing:

```sh
shasum -a 256 -c lope-<target>.tar.gz.sha256      # Linux: sha256sum -c lope-<target>.tar.gz.sha256
# Or filter the combined manifest portably on macOS and Linux:
grep "  lope-<target>\.tar\.gz$" SHA256SUMS | shasum -a 256 -c -
gh attestation verify lope-<target>.tar.gz --repo makakoo/lope

tar -xzf lope-<target>.tar.gz
cd lope-<target>
./install.sh                                       # defaults to --prefix "$HOME/.local" (no sudo needed)
lope --version
```

For a system-wide prefix such as `/usr/local`, separate the privileged file copy from unprivileged per-user skill registration so host integrations never write into `/root/`:

```sh
sudo ./install.sh --prefix /usr/local --skip-install
lope install --host all
```

Upgrading uses the same `./install.sh --prefix <PREFIX>` command (with transactional rollback if verification fails); existing host files are backed up automatically under `${XDG_STATE_HOME:-~/.local/state}/lope/install-backups/`. To roll back cleanly to legacy Python `0.17.3`, keep `/usr/local/bin/lope-py` or run any command with `LOPE_FORCE_PYTHON=1 lope ...`.

## Building from Source

Always build via `scripts/cargo` (respecting `rust-toolchain.toml`, working around Homebrew Rust preceding rustup on macOS `PATH`):

```sh
./scripts/cargo build --release                  # target/release/lope
./scripts/cargo test --workspace                 # unit, router, engine, and parity tests
./scripts/check-version.sh                       # product v2 + Python compat version sync
./tools/parity/dump-python && ./tools/parity/dump-rust && ./tools/parity/diff
LOPE_ROUTER_TRACE=1 target/release/lope review notes.md --consensus quorum
```

Runtime controls: `LOPE_FORCE_PYTHON=1` (delegate to Python rollback), `LOPE_FORCE_NATIVE=1` (uncovered → exit 2), `LOPE_PYTHON` (interpreter), `LOPE_ROUTER_TRACE=1`, `LOPE_STDERR_HARD_LIMIT` (bytes before killing runaway stderr, default `33554432` / 32 MiB; in-memory capture stays bounded at 512 KiB head+tail).

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
scripts/                          cargo wrapper, check-version.sh, bump-version.sh,
                                  package-release.sh, install-release.sh
install, skills/, commands/       host integrations (Claude, Codex, Gemini, OpenCode, Cursor, Vibe, Qwen, pi)
```

## Non-negotiables

- Treat [docs/reference/](docs/reference/) and `tools/parity/` as the compatibility baseline.
- Keep `~/.lope` state (`config.json` v1, `memory.db` `user_version = 1`, `~/.lope/runs/`) readable by Python Lope while rollback (`LOPE_FORCE_PYTHON=1` / `/usr/local/bin/lope-py`) and Makakoo's `gym_lope_gate` (DEC-8 / PM-1) remain active.
- Cross-repo changes in Glimmer (`makakoo-jungle`), Makakoo OS (`makakoo-os` / `~/MAKAKOO`), and `~/.lope` are tracked in [Spec 04 §8](docs/specs/04-interfaces-and-protocol.md#8-cross-repo-patch-sets).
