# SPRINT-SUPER-LOPE-V1

[Root](README.md) · [Audits](docs/audits/README.md) · [Specs](docs/specs/README.md) · [Sprints](docs/sprints/README.md)

## Origin

Plan revision 2 (2026-11-07). Deliver a Rust `lope` that is a behavioural drop-in for Python Lope at the PL-1 tag, for every caller in docs/audits/03 (Glimmer G1–G8, Makakoo M1–M8, the 17 skills, interactive shells). It uses a strangler router (DEC-1): unported invocations exec Python Lope, and coverage grows sprint by sprint behind parity tests (NFR14). One-shot validators run over pipes with Python's fail-closed completion policy, implemented with a realistic cleanup budget (DEC-2, DEC-4). State stays Python-readable at every boundary (NFR6). `/usr/local/bin/lope` changes only in S8, after the PL/PG/PM/PH patch sets land. The Herdr-derived PTY warm pool is Phase 10 (S9), optional and gated on FR12 evidence (OQ-4). Revision 1 is archived under archive/v1-2026-11-07-unverified and dispositioned in docs/audits/04-verification-errata-2026-11-07.md.

## Non-negotiables

1. Parity is defined by `docs/reference/` and the `tools/parity` dumps, never by prose.
2. Python Lope can read and operate on everything Rust writes (`config.json` version 1 with Python keys only; `memory.db` `user_version = 1`; `~/.lope/runs/`).
3. `~/.lope` stays a clean, importable git checkout (NFR7, DEC-8).
4. No edits to Glimmer, Makakoo or Python Lope from this repo. Cross-repo work ships as patch sets (Spec 04 §8).
5. Strictly serial. Each phase starts only when the previous phase's evidence file exists in `docs/evidence/`.

## Phases

### Phase 1: s0-preflight-workspace-and-router
**Goal:** Pin a committed parity baseline (PL-1) and ship a cargo-only workspace whose `lope` binary routes every invocation, provides native `version` / `--version` / `-V`, normalises Glimmer's `--consensus quorum` and `--rounds`, enforces the nesting guard, and includes the argparse-vs-clap parity tooling.
**Criteria:**
- `~/.lope` is clean at a recorded tag; `docs/reference/*` regenerated at that tag; `docs/evidence/pins.txt` records SHAs, toolchain and the Glimmer test baseline
- A1: parity dump diff is empty apart from allow-listed additive entries; `review --help` lists `--consensus --format --focus --brain-context`
- A2: delegated invocations are identical to direct Python (stdout, exit code, state; stderr normalised)
- `lope review x.md --consensus quorum --format markdown` and `lope negotiate g --rounds 3` no longer exit 2
- `lope version` p50 and p95 startup recorded on the Intel Mac
**Files:**
- `Cargo.toml`
- `rust-toolchain.toml`
- `crates/lope-compat/`
- `crates/lope-cli/`
- `tools/parity/`
- `docs/evidence/pins.txt`
- `docs/evidence/s0-router.txt`
**Tests:**
- `A1: tools/parity/dump-python && tools/parity/dump-rust && tools/parity/diff`
- `A2: cargo test -p lope-cli --test delegation_fidelity`

### Phase 2: s1-core-config-and-state
**Goal:** Port config (Python save semantics), redaction, the verdict parser, the runs registry and the run lock, and make `status` and `jobs` native with byte-compatible output.
**Criteria:**
- Starts only after `docs/evidence/s0-router.txt` exists
- A3: Rust config load+save is byte-equivalent to Python `save()`, and Python loads every Rust-written config
- A4: Rust and Python list, reap and prune each other's manifests identically; 3 dead pruned, 1 live untouched
- Python's tolerance of extra manifest fields is tested and recorded
- NFR6 rollback check passes
**Files:**
- `crates/lope-core/`
- `crates/lope-state/`
- `docs/evidence/s1-core-state.txt`
**Tests:**
- `A3: cargo test -p lope-core --test config_python_roundtrip`
- `A4: cargo test -p lope-state --test registry_python_interop`

### Phase 3: s2-supervised-runtime-and-completion
**Goal:** Build Tier 2 supervised one-shot execution over pipes with kill-then-confirm group cleanup, fail-closed UNVERIFIED semantics, timeout diagnostics, signal teardown and RAII temp resources.
**Criteria:**
- Starts only after `docs/evidence/s1-core-state.txt` exists
- A5: same-group lingering child is killed and the call is `ok` within 1 s of leader exit; repeated 50 times on the Intel Mac with zero false UNVERIFIED
- A6: escaped pipe holder gives UNVERIFIED with metadata, never a hang
- A7: timeout writes a 0600 redacted `.diag`; no forbidden manifest keys; Python `jobs list` still parses the run
- Group-confirmation latency distribution recorded and the default cleanup budget justified
**Files:**
- `crates/lope-runtime/`
- `docs/evidence/s2-runtime.txt`
**Tests:**
- `A5: cargo test -p lope-runtime --test completion_policy`
- `A6: cargo test -p lope-runtime --test escaped_pipe_holder`
- `A7: cargo test -p lope-runtime --test timeout_diagnostics`

### Phase 4: s3-providers-and-team
**Goal:** Port all validator sources (5 built-ins, 10 presets, generic subprocess and HTTP, Makakoo adapters, learned adapters), the curl parser, retry, latency and budgets, and make `team` and `configure` native.
**Criteria:**
- Starts only after `docs/evidence/s2-runtime.txt` exists
- A8: argv and stdin construction matches Python goldens for every built-in and preset; HTTP provider against a mock spawns 0 children; curl cases from `tests/test_curl_parser.py` pass; Makakoo adapter call works with a fake `makakoo`
- NFR6 rollback check passes after `team add` and `team remove`
**Files:**
- `crates/lope-providers/`
- `docs/evidence/s3-providers.txt`
**Tests:**
- `A8: cargo test -p lope-providers`

### Phase 5: s4-single-shot-review-and-memory
**Goal:** Make `ask`, `vote`, `compare`, `pipe`, `review` and `memory` native with exact output parity, stdout-as-report, verbatim `--focus`, and the unchanged `memory.db` schema.
**Criteria:**
- Starts only after `docs/evidence/s3-providers.txt` exists
- A9: single-shot goldens match Python, with isolation by default and `--require-all` failure
- A10: review reports identical across all five formats; SARIF validates; `--output` atomic
- A11: Glimmer G4 and G5 argv run correctly with stdout piped; SIGTERM reaps all children within 1 s
- A12: Rust and Python read and write `memory.db` interchangeably; `user_version` stays 1; 4 concurrent writers succeed
- FR12 diagnostics collection for OQ-4 started
**Files:**
- `crates/lope-engine/`
- `crates/lope-memory/`
- `docs/evidence/s4-review-memory.txt`
**Tests:**
- `A9: cargo test -p lope-engine --test single_shot_parity`
- `A10: cargo test -p lope-engine --test review_parity`
- `A11: cargo test -p lope-cli --test glimmer_argv_corpus`
- `A12: cargo test -p lope-memory --test python_interop`

### Phase 6: s5-makakoo-bridge
**Goal:** Make the brain flags native with ANSI and no-hits cleanup, a locked local-date journal append and `makakoo sync --file`, and write the PM patch descriptions.
**Criteria:**
- Starts only after `docs/evidence/s4-review-memory.txt` exists
- A13: fake `makakoo` table output is stripped and budgeted; `(no hits)` produces no block; journal bullet is locked and dated locally; `sync --file` runs
- `gym_lope_gate` dry run passes against `~/.lope`
- PM-1 to PM-6 written up with file:line and proposed diffs
**Files:**
- `crates/lope-makakoo/`
- `docs/evidence/s5-makakoo.txt`
**Tests:**
- `A13: cargo test -p lope-makakoo`

### Phase 7: s6-council-deliberation
**Goal:** Port `deliberate` exactly: positional template, seven stages, quorum, anonymisation, limits, and `--out` directory output.
**Criteria:**
- Starts only after `docs/evidence/s5-makakoo.txt` exists
- A14: `adr` and `prd` goldens match Python at all depths, including quorum failure and minority report
**Files:**
- `crates/lope-engine/src/deliberate/`
- `docs/evidence/s6-deliberate.txt`
**Tests:**
- `A14: cargo test -p lope-engine --test deliberation_parity`

### Phase 8: s7-sprint-engine-and-flow
**Goal:** Port `negotiate`, `execute`, `implement`, `audit`, `gate`, `check`, the healer, and `flow` with Python's flags, prompts, journals, trust rules and caps.
**Criteria:**
- Starts only after `docs/evidence/s6-deliberate.txt` exists
- A11: Glimmer G7 (`--rounds 3`) and G8 (`implement` roster prompt on a TTY) run correctly
- A15: negotiate, execute, implement, audit, gate and check goldens match Python; nesting guard exits 2; untrusted gates exit 3
- A16: Python flow cases and the three templates pass; cyclic graphs stop at caps
**Files:**
- `crates/lope-engine/src/sprint/`
- `crates/lope-flow/`
- `docs/evidence/s7-sprint-flow.txt`
**Tests:**
- `A15: ./scripts/cargo test -p lope-cli --test s7_sprint_flow_parity` (and `tools/parity/golden_run.py` strict, zero `"ignore"` hacks)
- `A16: ./scripts/cargo test -p lope-flow && ./scripts/cargo test -p lope-cli --test s7_sprint_flow_parity`

### Phase 9: s8-cutover
**Goal:** Land the cross-repo patch sets, install the Rust binary at `/usr/local/bin/lope` with `/usr/local/bin/lope-py` as rollback, remove the shell shadows, and verify Glimmer, Makakoo and the benchmarks.
**Criteria:**
- Starts only after `docs/evidence/s7-sprint-flow.txt` exists and OQ-2 and OQ-3 are answered
- PL-3, PG-1 to PG-8, PM-2 to PM-6, PH-1 and PH-2 merged in their own repos
- A17: `glimmer doctor` shows Lope OK; Glimmer suite has no new failures vs the NFR15 baseline; `makakoo setup lope` correct; `gym_lope_gate` dry run passes; `lope-py version` works
- M1 to M8 pass, with a Rust vs Python benchmark table recorded
- Fresh zsh `type lope` resolves to the Rust binary
**Files:**
- `/usr/local/bin/lope`
- `/usr/local/bin/lope-py`
- `docs/evidence/s8-cutover.txt`
**Tests:**
- `A17: ./scripts/cargo test -p lope-cli --test s8_cutover_parity && tools/parity/glimmer-baseline` (see detailed closeout plan in [`docs/sprints/S8B-remediation-and-cutover-closeout.md`](docs/sprints/S8B-remediation-and-cutover-closeout.md))

### Phase 9B: s8b-remediation-and-cutover-closeout

- Required before marking S7/S8 closed or evaluating S9 (`OQ-4`)
- Fixes S7 parity defects (`CallContext.implementation = true` for writers, untrusted gate exit `3`, `execute --manual` non-TTY EOF exit `3`, `implement --interactive`, `negotiate` progress + `.feedback.md`/`.rounds/`, strict `golden_run`, `SelfHealer` wiring, `LOPE_FLOW_WORKERS`)
- Fixes S8 hygiene (`install.rs` / `s8_cutover_parity.rs` hardcoded path removal, `FR39` Super Lope `reference.md` + `docs.rs` shadowing fix)
- Applies pending cross-repo patches (`PH-1` `~/.bashrc`, `PL-2`/`PL-3` `~/.lope`, `PG-1..PG-8` `makakoo-jungle`, `PM-2` `makakoo-os`, `PM-3` `MAKAKOO/AGENTS.md`) and executes `A17` + `M1–M8` benchmarks/live verification
- `M8: scripts/bench-intel-mac.sh`

### Phase 10: s9-pty-pool-and-daemon
**Goal:** Only if OQ-4 approves on FR12 evidence: add the opt-in feature-gated Tier 3 warm PTY pool in `loped`, with Herdr detection plus the stabiliser, Lope-authored blocker rules, a peer-UID-checked socket and a launchd environment per NFR16.
**Criteria:**
- Starts only after `docs/evidence/s8-cutover.txt` exists and OQ-4 is approved with evidence attached
- Default build remains cargo-only; `--features pty-pool` builds natively on the Intel Mac with Zig 0.16
- A18: socket modes and peer-UID rejection; 2 warm mock panes serve 4 calls without new spawns; blocker fixtures for existing rule ids act in under 250 ms
- Idle `loped` uses under 15 MB; M1 with the pool enabled for one provider shows no regression
**Files:**
- `crates-pty/`
- `vendor/`
- `docs/evidence/s9-pool.txt`
**Tests:**
- `A18: cargo test --features pty-pool -p lope-daemon`
