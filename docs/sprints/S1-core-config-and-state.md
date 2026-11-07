# S1 — Core, configuration and state

[Sprints](README.md) · Previous: [S0](S0-preflight-workspace-and-router.md) · Next: [S2](S2-supervised-runtime-and-completion.md)

## Goal

Port the state layer exactly, so that Rust and Python share `~/.lope` safely, and make `status` and `jobs` native.

## Preconditions

`docs/evidence/s0-router.txt` exists.

## Tasks

1. **`lope-core`:**
   - `$LOPE_HOME` paths;
   - `config.json` load, layering and env overrides, plus `save()` with Python semantics (FR6);
   - `super-lope/config.toml` reader with defaults only (FR7; nothing is written before PL-2);
   - `redaction.py` port (FR26);
   - verdict-block parser (FR25; repair prompts come in S4).
2. **`lope-state`:**
   - `runs/` registry, manifest schema and class logic, `_FORBIDDEN_MANIFEST_KEYS`;
   - process start and boot fingerprints as Python computes them;
   - `registry.lock` (FR8);
   - `run.lock` with `LOPE_RUN_LOCK*` and exit 75 (FR9).
3. **Native `status`** (the `Jobs:` section and team summary byte-compatible) and **`jobs list|reap|prune|kill`** with `--json`, `--dry-run` and `--stale`.
4. Mine `~/.lope/tests/test_config_layered.py`, `test_job_registry.py`, `test_jobs_cli.py`, `test_job_lifecycle.py`, `test_runlock.py`, `test_redaction.py` and `test_atomic_writes.py` for fixtures.

## Acceptance

- A3 and A4 pass, and the NFR6 rollback check passes.
- Verify, and record in evidence, whether Python tolerates extra manifest fields. This gates the FR12 placement.
- Evidence: `docs/evidence/s1-core-state.txt` with the coverage-table hash.
