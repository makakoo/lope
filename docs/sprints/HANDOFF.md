# Implementation handoff (living document)

Last updated 2026-11-07. Read this first when resuming the port.

## Where things stand

| Sprint | State | Evidence |
|---|---|---|
| S0 router, normalisation, parity tooling | **closed** | `docs/evidence/s0-router.txt`, `pins.txt`, `glimmer-baseline-2026-11-07.txt` |
| S1 config, state, native `status` + `jobs` | **closed** | `docs/evidence/s1-core-state.txt` |
| S2 supervised runtime, completion policy | **closed** | `docs/evidence/s2-runtime.txt` |
| S3 providers + team | **in progress** | see below |
| S4–S7 | not started | — |
| S8 cutover | not started; needs OQ-2/OQ-3 defaults and patch sets | — |
| S9 PTY pool | gated: needs OQ-4 approval based on ≥ 7 days of FR12 `.diag` evidence, which only accumulates after cutover. Cannot be finished in one session by design. | — |

## Verified building blocks (differential fuzzing against CPython / Python Lope)

- `lope_core::pyjson` dumps (4 flavours) and `loads_py` (incl. exact `JSONDecodeError` text): `tools/parity/pyjson_fuzz.py`, `pyjson_loads_fuzz.py`
- `lope_core::redaction`: `tools/parity/redact_fuzz.py`
- `lope_providers::{verdict, diagnostics, validity}`: `tools/parity/verdict_fuzz.py` (16k cases, 0 mismatches)
- DEC-11 normalisation: `tools/parity/normalise_fuzz.py`

## S3 port order (remaining)

Python sources in `~/.lope/lope/` (tag `super-lope-baseline-0.17.3`). Each module gets a
differential test before it is used by a native verb.

1. `runtime.py` → `lope_providers::budget` (RunBudget, CallLease, InvocationContext with registry + progress hooks; snapshot() JSON shape is part of `--json` output).
2. `latency.py` → `latency` (ledger `$LOPE_HOME/latency.json`, `json.dump(sort_keys, (",",":"))`, nearest-rank p90, fit_warnings text).
3. `progress.py` → `progress` (stderr `progress:` lines, timer `heartbeat:` thread, `format_runtime_summary`).
4. `invocation.py` → `invocation` (lease → call → outcome class → one typed retry; registry register/finish per call).
5. `request_plan.py` → `request_plan` (transport caps; DIRECT path exact; CHUNK path: native verbs must **bail out to delegation before any side effect** when the plan is not DIRECT; Python `ast` units are not portable).
6. Validators (`validators.py`, `generic_validators.py`): claude/codex/opencode/gemini/aider + generic subprocess (+ review profile) + learned adapters; HTTP providers and Makakoo adapters → bail out to delegation for now. Messages must match byte-for-byte (`_run_with_group_kill` texts, `diagnostic_suffix`).
7. `build_validator_pool` resolution order (learned → built-in → providers → KNOWN_CLIS generic → Makakoo).
8. `team list|enable|disable|remove` native (config writes via `lope_core::config::save`); `team add` (+ `curl_parser.py`), `team test|health`, `configure` (interactive) after.

## Native-verb pattern (S4 onward)

- Implement in `crates/lope-cli/src/native/<verb>.rs`; add the coverage entry in `crates/lope-compat/src/coverage.rs` with the exact arg ids supported.
- A native command may return "delegate" **before any output or state write** (e.g. `--synth`, `--brain-*`, http/makakoo providers, chunked plans). Wire this as an enum result in `native::run`, and have `main` exec Python in that case.
- Add golden cases to a `tools/parity/corpus/s4-*.json` with `default_route: native`; mock validators come from `golden_run.py` (`LOPE_MOCK_MODE=verdict` for VERDICT blocks).

## Gotchas learned

- Never type `\uXXXX` escapes into tool input; they get converted. Generate such text from Python with an explicit backslash.
- Python's non-MULTILINE `$` also matches before a final `\n`; translate to `(?=\n?\z)`, not `\z`.
- `round(sum([]), 3)` is the int `0`; JSON shape differences like this show up in `--json` diffs.
- Python Lope's closeout fails when `$LOPE_HOME` has a symlinked ancestor (F1/PL-5); golden sandboxes use realpaths.
- `memory.db` follows `$HOME`, not `$LOPE_HOME`.
- Build only via `scripts/cargo` (rustup 1.29 + Homebrew rustc on PATH).
