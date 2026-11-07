# Spec 03 — Requirements & acceptance

[Specs](README.md) · Previous: [02 — Use cases](02-use-cases-and-scenarios.md) · Next: [04 — Interfaces](04-interfaces-and-protocol.md)

**Labels used below:**
- **Parity:** behave exactly as Python Lope at the pinned commit, per [`docs/reference/`](../reference/) and [Audit 01](../audits/01-lope-python-audit.md).
- **Additive:** new behaviour that no existing caller can observe as a change.
- **Deviation:** an intentional difference, traced to a DEC entry.

## 1. Functional requirements

### 1.1 CLI, routing, configuration and state (FR1–FR9)

- **FR1 — CLI surface parity.**
  - All 23 verbs and their nested subcommands, with the same positionals, flags, aliases (`--structured`, `-o`, `lint`), choices, defaults, required-ness and mutually exclusive groups as the reference capture.
  - Usage errors exit 2, as argparse does.
  - No new global options (a global `-c/--config` would collide with `check`/`gate --config`).
  - Help text need not be byte-identical, with one exception: `review --help` must list `--consensus --format --focus --brain-context` (a Glimmer test checks this).
- **FR2 — Version (parity + additive).**
  - `lope version` exits 0, and the first match of `/v?([0-9]+\.[0-9]+(?:\.[0-9]+)?)/` on stdout is the product version (OQ-3; default `lope 2.0.0 (super-lope; compat 0.17.3)`).
  - Nothing that looks like a version may print earlier on stdout.
  - Additive: `lope --version` and `lope -V` print the same thing.
  - Completes in under 5 s under the launchd environment (Glimmer's companion timeout).
- **FR3 — Exit codes (parity).**
  - `0` success; `1` failure; `2` usage error or nested refusal.
  - `3` untrusted gate commands, or `--manual` without a TTY.
  - `75` run lock held; `124` run deadline exhausted or installer timeout.
  - `install` passes through the installer's exit code.
  - Death by signal yields 130 or 143 as seen by the shell.
- **FR4 — Nesting guard (parity).**
  - `LOPE_IMPLEMENTATION_DEPTH` > 0, or not an integer: every command prints `error: nested Lope orchestration is disabled inside an implementation writer` to stderr and exits 2.
  - Implementation writers receive the incremented depth (`processes.py:184-190`).
- **FR5 — Strangler routing (DEC-1).**
  - Native only when `(verb, subcommand, flag set)` is in the coverage table. Otherwise `exec` Python Lope with argv (after FR23 normalisation), env, cwd, stdio and exit status preserved.
  - `LOPE_FORCE_PYTHON=1` delegates everything; `LOPE_FORCE_NATIVE=1` makes uncovered invocations exit 2.
  - `LOPE_PYTHON` overrides the interpreter.
- **FR6 — `config.json` (parity).**
  - Read and layer: `~/.lope/config.json`, then `./.lope/config.json`, then env overrides (Audit 01 §4).
  - Reject (ignore) files without `"version": 1`.
  - Write only Python's recognised keys, omitting defaults as Python does, with exclusive `flock` on `config.json.lock`, then tmp, fsync and rename.
  - `validators` stays a list. Honour `LOPE_HOME`.
- **FR7 — Rust-only settings (additive, DEC-3).**
  - Optional `~/.lope/super-lope/config.toml` holds: completion cleanup budget, fail-open opt-in, the S9 pool, blocker and PTY settings, and per-provider tier opt-in.
  - The file is never written before PL-2 lands.
  - Absent file means defaults.
- **FR8 — Run registry (parity).**
  - `~/.lope/runs/{active,completed,work}` and `runs/registry.lock`, with the same manifest schema and class logic (`active, unresponsive, abandoned, cleanup_failed, ownership_unverified`).
  - `status` prints the same `Jobs:` section.
  - `jobs list|reap|prune|kill` take the same flags (`--json`, `--dry-run`, `--stale`).
  - Rust- and Python-written manifests are interchangeable.
- **FR9 — Run lock and deadlines (parity).**
  - `run.lock` semantics, `LOPE_RUN_LOCK`, `LOPE_RUN_LOCK_PATH`, `LOPE_RUN_LOCK_WAIT`, exit 75.
  - Run deadline (`--run-timeout`, `--allow-unbounded-run`, `LOPE_RUN_TIMEOUT`), exit 124.

### 1.2 Process runtime (FR10–FR14)

- **FR10 — Tier 2 supervised one-shot (DEC-2).**
  - Argv only; new session and process group.
  - Prompt delivery exactly per adapter: stdin then close, argv, or a 0600 file.
  - Separate stdout/stderr capture with head+tail truncation at 2 MiB / 512 KiB; argv policy limit 128 KiB.
  - Per-call deadline with `--respect-provider-timeout` semantics.
  - `LOPE_RUN_ID`, `LOPE_CALL_ID` and `LOPE_PROCESS_MARKER` are exported to children as Python does.
- **FR11 — Completion policy (DEC-4).** Implement Spec 01 §7:
  - Fail-closed `UNVERIFIED` with metadata when descendants survive the cleanup budget, or when pipes are held after the group exits.
  - `LOPE_RECOVER_UNVERIFIED_OUTPUT=1` exposes redacted output in a separate `unverified_output` field.
  - Default budget 2.0 s.
- **FR12 — Timeout diagnostics (additive; instruments D2).** On `provider_timeout`, record bytes received on each stream, seconds since the last byte, whether the leader was alive, the process-group member count, the leader's last state (`ps -o stat`), and a ≤ 4 KiB redacted stderr tail.
  - Everything goes in `~/.lope/runs/work/<call>.diag` (0600, removed by `jobs prune`).
  - Summary fields move into the manifest only once A4 proves that Python tolerates extra manifest fields.
  - No forbidden manifest keys are written.
- **FR13 — Signals.** SIGINT, SIGTERM or SIGHUP to `lope` terminate every owned process group (TERM, then KILL after 200 ms) and finish manifests before exiting. Glimmer's cancel signals only the Lope PID (`server.js:516`).
- **FR14 — Temp resources.** Prompt files are 0600 and scratch directories 0700. They are removed on success, error, panic or cancellation.

### 1.3 Providers (FR15–FR20)

- **FR15 — Built-in validators (parity).**
  - Classes `claude, opencode, gemini, codex, aider` with Python's exact argv, stdin and env behaviour (`LOPE_CLAUDE_ARGS`, `LOPE_CODEX_MODEL`, `LOPE_OPENCODE_ARGS`, `LOPE_WORKDIR`).
  - Output extraction: opencode JSON events, gemini JSON.
  - Presets `vibe, ollama, goose, interpreter, llama-cpp, gh-copilot, amazon-q, pi, qwen, agy`.
- **FR16 — Generic providers (parity).**
  - Subprocess fields: `name, type, command, stdin, prompt_wrapper, timeout, review_command, review_workdir, review_stdin`.
  - HTTP fields: `url, headers, body, response_path, prompt_wrapper, timeout, max_tokens, response_limit`.
  - HTTP runs in-process (T1).
- **FR17 — Makakoo adapter validators (parity).**
  - Discovery in `~/.makakoo/adapters/registered/*.toml` (`MAKAKOO_ADAPTERS_HOME`).
  - Invocation `makakoo adapter call <name> --timeout N` with the prompt on stdin; parse its ValidatorResult JSON.
  - `LOPE_MAKAKOO_ADAPTERS=0` disables. Lookup order as Python.
- **FR18 — Learned adapters and self-heal (parity).** Read and write `learned_adapters` in `config.json`; `LOPE_SELF_HEAL`.
- **FR19 — Team (parity).**
  - `team list|enable|disable|add|remove|test|health` with every `team add` flag, including `--from-curl` (port of `curl_parser.py`, validated against `tests/test_curl_parser.py` cases).
- **FR20 — Configure, latency, retry, budgets (parity).**
  - Interactive `configure`.
  - `latency.json` adaptive ceilings.
  - `retry_policy.py` (single transient retry, deterministic jitter, `LOPE_RETRY_JITTER`).
  - `request_plan.py`: `request_policy` (auto/direct/chunk), `max_calls`, `max_chunks`, `max_input_bytes`, `max_output_bytes`, remaining-time admission.

### 1.4 Single-shot, review and memory (FR21–FR27)

- **FR21 — `ask`, `vote`, `compare`, `pipe` (parity).** Flags per reference: `vote --options` required; `compare --criteria` optional; `pipe --require-all`; `--synth`, `--anonymous`, `--json`, `--context`.
- **FR22 — `review` (parity).**
  - One positional; a directory is allowed with `--divide files`.
  - `--consensus`/`--structured`, `--json`, `--min-consensus`, `--similarity`, `--format {text,json,markdown,markdown-pr,sarif}`, `--output/-o`, `--include-raw`, `--remember`, `--divide {files,hunks}`, `--roles`, pool flags, `--synth`, `--anonymous`, brain flags.
  - `--focus` is free text passed **verbatim**; minimality aliases expand as in `minimality.py:98-113`.
  - Stdout is the report, including Python's header lines; progress goes to stderr.
- **FR23 — Glimmer compatibility aliases (additive, DEC-11).**
  - `review … --consensus quorum` is treated as `--consensus`. The token `quorum` is consumed only when it immediately follows `--consensus` *and* the `file` positional is present elsewhere in argv.
  - `unanimous` and `any` exit 2 until OQ-1 is resolved.
  - `negotiate --rounds N` is treated as `--max-rounds N`.
  - Applied by the router before native or delegated execution.
- **FR24 — Findings and formats (parity).**
  - Finding extraction, merge and dedup thresholds (`--similarity`).
  - Consensus buckets and thresholds (`findings.py:923-940`) and `consensus_score`.
  - Formatters `text/json/markdown/markdown-pr/sarif` (`sarif.py`), and `exporters.py`.
- **FR25 — Verdict parsing (parity).** `---VERDICT---…---END---` parsed JSON-first with a YAML-ish fallback; `verdict_repair` with `LOPE_REPAIR_TIMEOUT`; `verdict_events` (`LOPE_VERDICT_EVENTS`).
- **FR26 — Output filters (parity).**
  - Redaction patterns from `redaction.py` exactly.
  - Caveman mode, on by default (`LOPE_CAVEMAN`).
  - `NO_COLOR` and `LOPE_NO_COLOR`.
  - `LOPE_PROGRESS`.
- **FR27 — `memory.db` (parity).**
  - Schema per [`python-lope-memory-db-schema.sql`](../reference/python-lope-memory-db-schema.sql), `user_version = 1`.
  - WAL with `synchronous=NORMAL`.
  - `LIKE` search; `LOPE_MEMORY=off`; `LOPE_MEMORY_DB`.
  - `memory stats|search|file|hotspots|forget|gates` with reference flags.

### 1.5 Makakoo bridge (FR28–FR30)

- **FR28 — `--brain-context` (parity + deviation DEC-10).**
  - Resolve `MAKAKOO_BIN`, then `makakoo` on `PATH`; run `makakoo search <q>` with a 30 s timeout.
  - **Strip ANSI and box-drawing characters; treat `(no hits)` as empty.**
  - Redact; budget `--brain-budget` (default 1,200 tokens × 4 characters, minimum 400).
  - Prepend Python's advisory block once.
  - No direct SQLite fallback.
- **FR29 — `--brain-log` (parity + deviation DEC-10).**
  - Require `MAKAKOO_HOME`; error as Python does when it is unset.
  - Append Python's bullet text (`[[Lope]] … [[Makakoo OS]]`, every line starting `- `) to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md`, **using the local date and an exclusive file lock**.
  - Then run `makakoo sync --file <journal>` (15 s timeout, failures ignored as in Python).
- **FR30 — Auto-memory (parity).** `LOPE_BRAIN_AUTOMEMORY=1` writes `$MAKAKOO_HOME/data/auto-memory/lope-<name>.md`.

### 1.6 Council, sprint and flow (FR31–FR36)

- **FR31 — `deliberate` (parity).**
  - Positional `{adr,prd,rfc,build-vs-buy,migration-plan,incident-review}` and scenario (path or `-`).
  - `--depth`, `--out DIR` (default `lope-runs/<ts>-<template>/`, `LOPE_RUN_OUTPUT_DIR`), `--no-anonymize`, `--minority-report`, `--human-questions`, `--json`, brain flags.
  - Seven stages, with `quick` skipping critique and revision. Quorum `expected // 2 + 1`. Labels `Response A…AA`. Limits 48/32/64 KB; 5 workers.
- **FR32 — `negotiate` (parity).**
  - `--max-rounds` (default 3), `--context`, `--context-file`, `--domain`, `--out`, brain flags.
  - Domain section names (`models.py:139-155`).
  - Slug rule (`negotiator.py:414-418`); `.rounds` sidecar; negotiate env knobs (`LOPE_NEGOTIATE_*`).
- **FR33 — `execute` and `implement` (parity).**
  - `execute --phase --manual --gates --gate-config --trust` (3 retries per phase).
  - `implement --agents --escalate-to --phase --gates --gate-config --trust --dry-run --interactive`.
  - Interactive roster prompt on a TTY; implementation runner semantics; `LOPE_EVIDENCE_GATE`, `LOPE_YES_RUN_PROJECT_COMMANDS`.
- **FR34 — Audit, gates, trust (parity).**
  - `audit` writes `$LOPE_HOME/journal/YYYY_MM_DD.md` by default (`--no-journal` disables).
  - `gate save|check` and `check` use `.lope/rules.json` and baselines, `--remember` and `--trust`.
  - Trust store `trusted_gates.json`, `LOPE_TRUST_GATES`, exit 3.
- **FR35 — Healer and heal journal (parity).** `healer.py` behaviour; `journal.jsonl` heal events.
- **FR36 — Flow (parity).**
  - Node types `start, agent, review, judge, script, gate, exit`, plus aliases and `shape=` mappings.
  - Interpolation `$task`, `$VAR`, `{node.key}` (keys `outcome, detail, out, out_ref, verdict, rationale`).
  - Caps: `max_visits` (default 3), `max_node_visits` (default `max(50, 8 × nodes)`), `max_model_calls`, retries 3.
  - Templates `consensus, judge-loop, review-gate`.
  - Verbs `run, validate/lint, render (-o, -T), init, list`; `LOPE_FLOW_WORKERS`.

### 1.7 Host integration (FR37–FR39)

- **FR37 — `install` (parity).**
  - `--host {claude,codex,gemini,opencode,cursor,vibe,qwen,pi,all}` produces the same artifacts as `~/.lope/install`, including gemini `commands/lope`, skills and commands.
  - Shipped skill and command content invokes `lope …`, never `python3 -m lope` (PL-3).
- **FR38 — `update`/`upgrade`.** Delegated to Python until OQ-2 is decided. Once decided, native with the same flags (`--dry-run --method --host --skip-install --allow-dirty`), or with documented replacements.
- **FR39 — `docs` (parity).** Prints the reference documentation, updated for Super Lope.

### 1.8 Post-cutover, gated by OQ-4 (FR40–FR43, S9)

- **FR40 — `loped` daemon.**
  - Socket under `~/.lope/super-lope/run/` (directory 0700, socket 0600), plus a `getpeereid` UID check (new code).
  - Runs as launchd `com.makakoo.lope`, or as a Makakoo `kind="service"` plugin, with the environment required by NFR16.
- **FR41 — Warm PTY pool.**
  - Only for providers that opt in. PTY + `ghostty-vt` (feature `pty-pool`).
  - Readiness comes from Herdr detection **plus** a port of the pane-layer stabiliser (`src/pane/agent_detection.rs`).
  - Prompts are delivered with bracketed paste or temp-file reference; responses are extracted from the raw byte stream, not the wrapped screen grid.
  - Silent fallback to T2.
- **FR42 — Blocker policy.**
  - References only rule ids that exist (Audit 02 §2).
  - Dismiss keys are authored per agent and version in Lope's own rule file.
  - Fixtures live in this repo; Herdr's `AGENTS.md:201` forbids them upstream.
  - Read-only verbs never auto-approve tool permissions (L9).
- **FR43 — Additive verbs.** `lope daemon {start,stop,restart,status}`, `lope pool {list,warm,drain,reset}`, `lope detect <capture> [--agent]`. They exist only in `pty-pool` builds.

## 2. Non-functional requirements

- **NFR1 — Build.** The default build is cargo-only (no Zig, no Python, no network beyond crates.io). The toolchain is pinned and invoked through rustup. x86_64 is built natively on the Intel Mac. `pty-pool` adds Zig 0.16.
- **NFR2 — Performance** (Intel i5-8257U):
  - `lope version`: p50 < 10 ms, p95 < 25 ms over 20 runs. Python baseline is 1.00–1.25 s.
  - Router overhead on delegated invocations ≤ 15 ms.
  - Peak RSS of the `lope` process in a 5-validator fan-out, excluding children: < 30 MB.
  - (S9) idle `loped` with 0 panes: < 15 MB.
- **NFR3 — No sleep-polling while waiting on I/O.** The only bounded polling allowed is the post-exit group-emptiness confirmation (Spec 01 §7).
- **NFR4 — No shell for validators.** Gate commands and flow `script` nodes follow Python's execution model and trust rules.
- **NFR5 — Manifest privacy.** None of `prompt, prompt_text, response, response_body, raw_response, secret, credentials, argv, environment, env` may appear in manifests (`jobs.py:37-48`). Diagnostic tails live only in 0600 `.diag` files.
- **NFR6 — Rollback safety.** At every sprint boundary, Python Lope operates correctly on everything Rust wrote: `config.json`, `memory.db` (`user_version` stays 1), `runs/`, `journal/`, `latency.json`, `trusted_gates.json`.
- **NFR7 — `~/.lope` stays a clean git tree.** Rust creates nothing untracked outside gitignored paths. `~/.lope/super-lope/` is used only after PL-2.
- **NFR8 — Atomic writes.** `config.json` follows Python's sidecar lock + tmp + fsync + rename. Reports written with `--output` are written atomically.
- **NFR9 — SQLite.** Bundled SQLite; WAL; `synchronous=NORMAL`; a 5 s busy timeout. Four concurrent writers produce no `database is locked`.
- **NFR10 — Deterministic offline tests.** A1–A16 run offline with mock CLIs, a mock HTTP server and a fake `makakoo` binary.
- **NFR11 — Platform isolation.** OS-specific code lives only in `lope-runtime::platform::{macos,linux}` (and `lope-daemon` for S9).
- **NFR12 — Licensing.** Super Lope is MIT (like Lope and Makakoo). Herdr-derived files carry Apache-2.0 attribution in a `NOTICE` plus modified-file notes. `vendor/portable-pty` and `libghostty-vt` keep their MIT `LICENSE` files. Do not describe the project as "clean-room".
- **NFR13 — Evidence hygiene.** No secrets and no raw `config.json` values in `docs/evidence/`.
- **NFR14 — Parity harness.**
  - `tools/parity` dumps Python's argparse tree (by walking `_actions`) and the Rust clap tree to JSON and diffs them.
  - Golden runs execute both binaries against the same mock validators and compare stdout, stderr (normalised), exit code and written state.
  - Python Lope's own test cases (`~/.lope/tests/`) are mined for fixtures.
- **NFR15 — Glimmer baseline.** Regressions are measured against the pre-cutover Glimmer test results, not against "all green". Known pre-existing failures:
  - the `v0.17.2` pin (`watched-council.test.js:290`)
  - zsh `PIPESTATUS` exit capture
  - the slot leak (`jobs.js:96-106`)
  - plus anything recorded in `docs/evidence/pins.txt`
- **NFR16 — Service environment (S9).** Any launchd-managed process loads `~/.env` (`set -a; . ~/.env; set +a`), sets `MAKAKOO_HOME`, and builds a `PATH` that includes `~/.local/bin`, the active nvm node bin directory, `/usr/local/bin` and `/opt/homebrew/bin`. This mirrors `makakoo-platform/src/macos.rs:41, 67-74` and `service_env.rs`.

## 3. Non-goals for v2.0

- **L1** No GUI; Glimmer is the GUI.
- **L2** No embedded model inference.
- **L3** No PyO3 or embedded Python. Delegation is `exec`, and only during the strangler phase.
- **L4** No automatic commits or pushes.
- **L5** No remote execution (`--machine`): Herdr remote requires Herdr on the host, and no host configuration is verified.
- **L6** No `lope attach` TUI: Herdr's renderer is not extractable. Revisit after S9.
- **L7** No FTS index in `memory.db`: it would need triggers or a schema change that endangers rollback.
- **L8** No short role-lens alias map (`product`, `tax`, …): Glimmer sends prose, and `--roles` already exists.
- **L9** No auto-approval of tool permissions in read-only verbs.
- **L10** No breaking change to any Python-readable file format.

## 4. Automated acceptance tests

| ID | Target | Pass criteria | Sprint |
|---|---|---|---|
| **A1** | CLI surface + version | The parity dump diff (Python argparse vs Rust clap) is empty apart from documented additive entries. `version`, `--version` and `-V` exit 0. The first `N.N` on stdout is the product version. `review --help` lists the four Glimmer-checked flags. | S0 |
| **A2** | Delegation fidelity | For a corpus of uncovered invocations, Rust → Python delegation yields identical stdout, exit code and written state compared with direct Python (normalised stderr). `LOPE_FORCE_PYTHON` and `LOPE_FORCE_NATIVE` behave as specified. | S0 |
| **A3** | Config round-trip | Rust load+save of fixture configs (including the live config's *shape*) is byte-equivalent to Python's `save()`. Python loads every Rust-written config. Unknown keys behave as in Python. | S1 |
| **A4** | Registry, status, jobs | Rust and Python list, reap and prune each other's manifests identically. Seed 3 dead-PID manifests and 1 live: after `jobs prune --stale`, the 3 are pruned and the live one is untouched. `status` `Jobs:` output matches Python. | S1 |
| **A5** | Completion policy | (a) Mock validator prints a verdict, forks a same-group child that sleeps 30 s holding stdout, and exits 0 → child killed, group confirmed empty, `ok` within 1 s of leader exit. (b) Child that ignores SIGTERM → killed by SIGKILL, `ok`. (c) Unkillable or held group (simulated) → `UNVERIFIED` with metadata; output only with `LOPE_RECOVER_UNVERIFIED_OUTPUT=1`. | S2 |
| **A6** | Escaped pipe holder | A grandchild that calls `setsid` and holds stdout → `UNVERIFIED (output_drain_timeout)` with metadata, never a hang. | S2 |
| **A7** | Timeout diagnostics | A provider that times out writes a 0600 redacted `.diag` with all FR12 fields. The manifest holds no forbidden keys. Python `jobs list` still parses the run. `jobs prune` removes the `.diag`. | S2 |
| **A8** | Providers | Golden argv/stdin construction for every built-in and preset vs Python. HTTP provider against a local mock with 0 child processes. `--from-curl` cases from `tests/test_curl_parser.py`. Makakoo adapter discovery and call with a fake `makakoo`. Learned adapter round-trip. | S3 |
| **A9** | Single-shot verbs | `ask/vote/compare/pipe` goldens vs Python with 3 mock validators, one failing. Isolation by default; `--require-all` fails. | S4 |
| **A10** | Review pipeline | Same mock outputs give identical `text/json/markdown/markdown-pr/sarif` reports vs Python. SARIF validates against 2.1.0. `--output` is atomic. Stdout carries only the report and header. | S4 |
| **A11** | Glimmer argv corpus | All G1–G8 argv shapes, including `--consensus quorum` and `--rounds 3`, parse and run correctly through the router, both natively and delegated. Stdout/stderr split is verified with stdout piped (as `tee` does). SIGTERM to `lope` reaps all children within 1 s. | S4 (review), S7 (negotiate, implement) |
| **A12** | `memory.db` | Rust writes and Python reads, and vice versa, for all five tables. `user_version` stays 1. Four concurrent writers succeed. | S4 |
| **A13** | Brain bridge | With a fake `makakoo` emitting an ANSI box table: stripped, budgeted, prepended once; `(no hits)` produces no block. Journal append is locked, uses the local date and `- ` bullets, and is followed by `sync --file`. Unset `MAKAKOO_HOME` behaves as Python. | S5 |
| **A14** | Deliberation | Goldens vs Python for `adr` and `prd` at all depths. Anonymisation, quorum failure and minority report all match. | S6 |
| **A15** | Sprint engine | `negotiate` (domain sections, slug, `.rounds`), `execute`, `implement` (roster prompt under a PTY harness, depth export), `audit` journal, `gate`/`check` trust and exit 3, and the nesting guard exit 2 all match Python goldens. | S7 |
| **A16** | Flow | Python's flow test cases and the three templates: `validate`, `render`, `run`; caps terminate cyclic graphs; interpolation matches. | S7 |
| **A17** | Cutover integration | `glimmer doctor` shows Lope OK. The Glimmer suite shows no new failures vs baseline. `makakoo setup lope` status is correct with PM-2. A dry run of `gym_lope_gate` passes. `/usr/local/bin/lope-py version` still works. | S8 |
| **A18** | Daemon / pool / blocker (gated) | Socket modes and peer-UID rejection. Two warm mock TUI panes serve 4 sequential calls with no new spawns. Blocker fixtures for existing rule ids trigger the authored actions in < 250 ms. | S9 |

## 5. Manual acceptance checks

- **M1** Live `lope review README.md --consensus --format markdown` with the real team (`codex, claude, grok, agy, kimi, claude-safe-review`), run Rust vs `LOPE_FORCE_PYTHON=1`. Compare findings and outcomes; record any `UNVERIFIED` or `provider_timeout`.
- **M2** Glimmer GUI: **Council these notes** with a Tax lens. The report file is complete; progress is visible in the tab.
- **M3** Glimmer headless `glimmer review <file>`: `.jungle/reviews/<id>.md` is complete; cancel tears down children.
- **M4** Glimmer Plan → Build in a child worktree (`--rounds 3`, roster prompt).
- **M5** `gym_lope_gate` runs in its nightly window after cutover and passes.
- **M6** `--brain-context "Makakoo" --brain-log`: `makakoo search "Lope"` finds the new bullet under today's local date.
- **M7** Rollback drill: switch to `lope-py`, then exercise status, jobs, review `--remember`, memory and team on Rust-written state.
- **M8** Benchmarks on the Intel Mac: startup p50/p95, router overhead, fan-out RSS, wall-clock of a 3-validator review, Rust vs Python.
