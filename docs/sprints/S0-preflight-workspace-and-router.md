# S0 — Pre-flight, workspace and router

[Sprints](README.md) · Next: [S1](S1-core-config-and-state.md)

## Goal

Pin a committed parity baseline, stand up a cargo-only workspace, and ship a `lope` binary that:
- routes every invocation (native or delegated);
- fixes D4 and D6 for every verb through argv normalisation;
- includes the parity tooling every later sprint depends on.

## Pre-flight (before any code)

1. **PL-1:** Sebastian commits and tags the `~/.lope` working tree (or discards it). Record the tag and SHA. `git -C ~/.lope status --porcelain` must be empty.
2. Regenerate `docs/reference/*` at that tag (the commands are in the header of each capture).
3. Record in `docs/evidence/pins.txt`:
   - the Herdr, Makakoo and Glimmer SHAs and versions;
   - the toolchain (`which -a cargo`, `rustup show`);
   - the Glimmer test-suite baseline results (NFR15), including the known failures.
4. Choose and pin the toolchain in `rust-toolchain.toml`. Build scripts invoke `~/.cargo/bin/cargo` explicitly.

## Tasks

1. **Workspace:** the crates from Spec 01 §3 without `crates-pty/`. Shared deps aligned with makakoo-os. Release profile.
2. **`lope-compat` router** (FR5):
   - normalisation (FR23, DEC-11);
   - nesting guard (FR4) with Python's exact message;
   - coverage table;
   - `exec` delegation with `LOPE_PYTHON`, `LOPE_FORCE_PYTHON`, `LOPE_FORCE_NATIVE`.
3. **Native `version`** (FR2) plus `--version` and `-V`.
4. **`tools/parity`:**
   - (a) a Python script that walks argparse `_actions` for every verb and subcommand and dumps JSON: options, dests, nargs, choices, defaults, required, mutually exclusive groups;
   - (b) the same dump from the Rust clap tree;
   - (c) a diff with an allow-list for additive entries;
   - (d) a golden-run harness that runs `lope` and `LOPE_FORCE_PYTHON=1 lope` against mock validators and compares stdout, normalised stderr, exit code and a state snapshot.
5. **Clap grammar for all 23 verbs**, generated from or checked against the parity dump. Parsing only; execution is delegated.

## Acceptance

- A1: the argparse ↔ clap diff is empty apart from allow-listed additive entries; version behaviour per FR2; `review --help` lists the four Glimmer-checked flags.
- A2: the delegation corpus is identical to direct Python.
- `lope review x.md --consensus quorum --format markdown` and `lope negotiate g --rounds 3` no longer exit 2 (they delegate normalised argv).
- NFR2: `lope version` startup p50/p95 recorded.
- Evidence: `docs/evidence/s0-router.txt`.

## Note

From this point the binary can optionally be dogfooded ahead of S8. Installing it at `/usr/local/bin/lope` before PG-1 is unsafe, because `glimmer install` would overwrite it.

## Status (2026-11-07)

**Code complete; gate open.** Evidence: [`docs/evidence/s0-router.txt`](../evidence/s0-router.txt), [`docs/evidence/pins.txt`](../evidence/pins.txt).

| Item | State |
|---|---|
| PL-1 (`~/.lope` committed and tagged) | **Open (Sebastian).** Baseline is HEAD `9cffa55` plus a 15-file uncommitted diff, fingerprinted in `pins.txt`. Re-run `tools/parity/dump-python --update`, A1 and A2 at the tag. |
| `docs/reference/*` at the tag | Waits on PL-1 (current captures are from the same dirty tree). |
| NFR15 Glimmer baseline | **Not recorded.** The companion test spawns a headless `lope review` (likely real validators). Needs an OK or a fake-`lope` run. |
| Workspace, toolchain pin | Done. `scripts/cargo` is the build entry point (rustup 1.29 + Homebrew rustc, see the script). |
| Router: guard, normalisation, coverage, delegation | Done (`crates/lope-compat`). |
| Native `version`, `--version`, `-V` | Done. |
| Clap grammar for all 23 verbs | Done. Generated at build time from the argparse capture; the build fails on anything it cannot map. |
| `tools/parity` (a)–(d) | Done. |
| A1 | Pass: 48 commands, 347 arguments, 0 differences, empty allow-list. |
| A2 | See the evidence file. |
| NFR2 startup | Measured under load (load1 31): `lope version` p50 18.4 ms / p95 21.0 ms, where `/usr/bin/true` measured the same way is p50 15.6 ms. Lope's own cost is about 3 ms, against Python's 622 ms. The absolute budget (p50 < 10 ms) and router overhead (≤ 15 ms) cannot be judged on this loaded machine; re-measure when it is quiet. |
| Normalisation fuzz | Pass: 400 random argv against the real argparse, 0 failures (`tools/parity/normalise_fuzz.py`). |

**Deviations from the plan text:**
- A2's test lives in `lope-cli` (`cargo test -p lope-cli --test delegation_fidelity`), because it needs the `lope` binary. The master sprint is updated.
- The nesting guard runs before normalisation, as in Python (Spec 01 §5 updated).
- `LOPE_ROUTER_TRACE=1` was added (stderr only) for evidence and debugging.
- Added `tools/parity/normalise_fuzz.py`, a differential fuzz of DEC-11 against the real argparse.

**Findings handed to later sprints** (details in the evidence file):
- Python Lope closeout bug when `$LOPE_HOME` has a symlinked ancestor (PL-5; S1/S2 must decide whether Rust reproduces it).
- `memory.db` lives in `$HOME/.lope`, not `$LOPE_HOME` (S4, FR27).
- Python modules disagree on an empty `LOPE_HOME` (S1, FR6/FR8).
