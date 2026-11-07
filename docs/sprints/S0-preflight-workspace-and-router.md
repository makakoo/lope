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
