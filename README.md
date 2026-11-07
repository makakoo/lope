# Super Lope (`super-lope`) — Rust successor to Python Lope

**Status: plan revision 2 (2026-11-07).** Revision 1 was audited against the live repositories and contained fabricated or wrong claims (invented CLI flags, schemas, paths, defect evidence and Herdr capabilities). It is preserved unchanged in [`archive/v1-2026-11-07-unverified/`](archive/v1-2026-11-07-unverified/). Every claim from it is dispositioned, with evidence, in the [verification errata](docs/audits/04-verification-errata-2026-11-07.md).

**Implementation: S0 code complete, S0 gate open (2026-11-07).** The router, normalisation, nesting guard, native `version` and the parity tooling are built and tested (A1 pass, A2 see [`docs/evidence/s0-router.txt`](docs/evidence/s0-router.txt)). S0 cannot close until PL-1 (commit/tag `~/.lope`) is done and A1/A2 are re-run at that tag.

**Goal:** replace Python Lope (`/Users/sebastian/.lope`) with a Rust `lope` binary that is behaviourally compatible with today's callers — Glimmer (`makakoo-jungle`), Makakoo OS (`makakoo-os` / `~/MAKAKOO`), the 17 installed Lope skills, and interactive shells — while fixing verified runtime defects. Selected pieces of Herdr (`/Users/sebastian/projects/apps/herdr`) are reused only where they earn their cost (see [Spec 01 §6](docs/specs/01-product-and-architecture.md#6-herdr-reuse-what-is-taken-and-what-is-not)).

## Live pins (verified 2026-11-07)

| Component | Verified state |
|---|---|
| Machine | Intel Core i5-8257U (4 cores / 8 threads), 16 GB, x86_64 macOS, Homebrew prefix `/usr/local`. Homebrew `cargo 1.89.0` precedes `~/.cargo/bin/cargo` on `PATH`. `zig` not installed. |
| Python Lope | `~/.lope`, branch `main`, HEAD `9cffa55` (`__version__ = "0.17.2"`) **plus 15 uncommitted files** that bump to `0.17.3` and rework completion handling. 62 `.py` files (53 in `lope/`, 9 in `lope/flow/`), 27,322 lines. MIT. |
| Herdr | `master` @ `ee5bfb50`, `0.9.5`, Apache-2.0, `rust-toolchain.toml` = `1.96.1`. `src/` 291,081 lines; `crates/ghostty-vt` (12,331 lines) is a binding to vendored `libghostty-vt` built with **Zig 0.16**; 22 detection manifests (1,798 lines). |
| Makakoo OS | `makakoo-os` @ `2e1bfe7`, `0.4.0`, MIT, 6 workspace crates. `makakoo` at `~/.local/bin` and `/usr/local/bin` (Cellar 0.4.0). `MAKAKOO_HOME=/Users/sebastian/MAKAKOO`. |
| Glimmer | `makakoo-jungle` @ `8a541055`; `/usr/local/bin/glimmer` → `makakoo-jungle/bin/glimmer`. |
| `lope` entry points | `/usr/local/bin/lope` (3-line sh shim → `/usr/bin/python3 -m lope`, written by `glimmer install`), `~/bin/lope` (bash wrapper), and `alias lope=…` in `~/.zshrc:178` and `~/.bashrc:33`. |

## What changed from revision 1 (summary)

1. **Rationale corrected.** The Darwin reaping shortcut blamed for D1 is uncommitted work-in-progress, not 0.17.3 @ `9cffa55`; 0 of 139 live manifests show `descendants_live`. The dominant live failure is `provider_timeout` (claude 18, agy 5, grok 4), whose cause is unproven. See [Audits](docs/audits/README.md#defect-matrix).
2. **Compatibility is defined by the real CLI**, captured in [`docs/reference/python-lope-0.17.3-cli-help.txt`](docs/reference/python-lope-0.17.3-cli-help.txt), not by an invented contract. Real state paths (`~/.lope/runs/`), real `memory.db` schema, real config rules (`"version": 1`, Python drops unknown keys on save).
3. **Strangler cutover.** The Rust binary delegates any verb/flag set it has not yet ported to Python Lope, so parity is proven verb by verb instead of in one big-bang switch.
4. **One-shot validators stay on pipes.** Herdr's manifests model interactive TUIs only; `--print`/`exec` modes are not detectable. The PTY warm pool, `ghostty-vt` (Zig build), daemon and blocker policy move to a gated post-cutover sprint (S9).
5. **Cross-repo work is explicit.** Glimmer, Makakoo and the Python repo all need small patches (manifest path, shim writer, gym gate imports, skills). They are listed as patch sets PG/PM/PL/PH in [Spec 04 §8](docs/specs/04-interfaces-and-protocol.md#8-cross-repo-patch-sets).

## Documentation map

```text
README.md                         this file
SPRINT-SUPER-LOPE-V1.md           executable master sprint (lope phase format), plan revision 2
docs/
├── audits/                       verified ground truth
│   ├── README.md                 pin table, defect matrix, verification method
│   ├── 01-lope-python-audit.md
│   ├── 02-herdr-rust-audit.md
│   ├── 03-makakoo-and-glimmer-seams-audit.md
│   └── 04-verification-errata-2026-11-07.md   every revision-1 claim → verdict → evidence
├── reference/                    machine-captured baselines (regenerate in S0)
│   ├── python-lope-0.17.3-cli-help.txt
│   ├── python-lope-memory-db-schema.sql
│   └── python-lope-env-vars.txt
├── specs/
│   ├── README.md                 ID registry, decisions, open questions
│   ├── 01-product-and-architecture.md
│   ├── 02-use-cases-and-scenarios.md
│   ├── 03-requirements-and-acceptance.md
│   └── 04-interfaces-and-protocol.md
└── sprints/                      S0–S9, strictly serial
archive/v1-2026-11-07-unverified/ revision 1, unmodified
Cargo.toml, rust-toolchain.toml   workspace (pinned 1.96.1)
crates/                           lope-core, lope-compat (router), lope-cli (`lope` binary);
                                  the other crates are empty placeholders until their sprint
tools/parity/                     argparse ↔ clap dump + diff (A1), golden-run harness (A2)
scripts/cargo                     build entry point (see below)
docs/evidence/                    per-sprint evidence files
```

## Building and testing

Always build through `scripts/cargo`. On this Mac, Homebrew's cargo/rustc 1.89 come before rustup's on `PATH`, and rustup 1.29 no longer puts the toolchain first when it runs cargo. A bare `~/.cargo/bin/cargo` therefore still compiles with rustc 1.89, which `rust-version = "1.96"` rejects.

```sh
scripts/cargo build --release            # target/release/lope
scripts/cargo test --workspace           # unit + router tests; A2 runs real Python Lope (slow)
tools/parity/dump-python && tools/parity/dump-rust && tools/parity/diff     # A1
LOPE_ROUTER_TRACE=1 target/release/lope review notes.md --consensus quorum  # show routing
```

Router controls: `LOPE_FORCE_PYTHON=1` (delegate everything), `LOPE_FORCE_NATIVE=1` (uncovered → exit 2), `LOPE_PYTHON` (interpreter), `LOPE_ROUTER_TRACE=1`.

## Hard rules for anyone executing this plan

- Treat the [reference captures](docs/reference/) as the parity baseline and regenerate them at the pinned commit in S0. Do not reintroduce claims from `archive/`.
- `~/.lope` stays installed and importable until Makakoo patch PM-1 lands (the nightly SANCHO `gym_lope_gate` imports `lope.*` directly).
- Never write state Python Lope cannot read back (rollback must work at every sprint boundary).
- No edits to Glimmer, Makakoo or Python Lope from this repo; those changes ship as the patch sets in Spec 04 §8, through their own repos.
