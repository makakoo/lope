# Spec 03: Requirements & Acceptance Tests

[Specs Index](README.md) · Previous: [02 — Use Cases & Scenarios](02-use-cases-and-scenarios.md) · Next: [04 — Interfaces & Protocol](04-interfaces-and-protocol.md)

---

## 1. Functional Requirements (`FR1`–`FR40`)

### 1.1 CLI, Config & Drop-In Compatibility (`FR1`–`FR8`)
- **`FR1` (23-Verb Drop-In CLI Parity)**: `lope` must accept all 23 existing subcommands (`negotiate`, `execute`, `implement`, `audit`, `ask`, `review`, `vote`, `compare`, `pipe`, `team`, `deliberate`, `memory`, `gate`, `check`, `status`, `jobs`, `configure`, `install`, `update`, `upgrade`, `version`, `docs`, `flow`) with identical positional arguments, short/long flags, and exit codes (`0` = success/PASS, `1` = runtime/validation failure, `2` = CLI usage/admission refusal).
- **`FR2` (Version Flag & Subcommand Parity)**: `lope version`, `lope --version`, and `lope -V` must all exit `0` and output a version line matching `glimmer doctor`'s semver check (`>= 0.17.3`, e.g., `lope 2.0.0 (super-lope; compat 0.17.3)`).
- **`FR3` (`~/.lope/config.json` & `lope.toml` Compatibility)**: Read and write `~/.lope/config.json` preserving all existing keys (`primary`, `validators`, `parallel`, `timeout`, `request_policy`, `max_chunks`, `max_calls`, `max_input_bytes`, `providers`) while optionally supporting `~/.lope/config.toml`.
- **`FR4` (Side-by-Side Fallback Shim `lope-py`)**: During installation and cutover (`S8`), preserve `/usr/local/bin/lope-py` pointing to `~/.lope` (`python3 -m lope`) so rollback is a single symlink switch if ever needed.
- **`FR5` (New Runtime Subcommands)**: Add four native subcommands for Herdr runtime features:
  - `lope daemon {start,stop,restart,status}` — manage the background `loped` server (`~/.lope/run/lope.sock`).
  - `lope pool {list,warm,drain,reset}` — inspect and manage warm agent PTY panes.
  - `lope attach [<run-id>]` — open the live Ratatui split-pane TUI for active or recent runs.
  - `lope detect <pane-or-file> [--agent <id>]` — run the 22-agent screen detector against a live pane or captured ANSI/text file for diagnostics.
- **`FR6` (Run Registry & Manifest Compatibility)**: Maintain `~/.lope/state/sprints/` JSON manifests compatible with `lope status` and `lope jobs {list,show,prune --stale,reconcile}`.
- **`FR7` (Budget, Lease & Admission Planning)**: Port `request_plan.py` and `runtime.py` budget leases (`max_calls`, `max_chunks`, `max_input_bytes`, `remaining_seconds` check before reserving a call lease).
- **`FR8` (Typed Retry & Circuit Breaker)**: Port `retry_policy.py` so transient failures (`rate_limited`, `service_unavailable`, `connection_reset`) retry at most once within remaining budget using deterministic jitter, while non-retryable failures (`auth_error`, `input_limit`, `tool_permission_denied`, `blocked`) never retry.

### 1.2 PTY Runtime, Screen Detection & Reaping (`FR9`–`FR16`)
- **`FR9` (Leader-Completion Authority on macOS/POSIX — Fixes `D1` & `D5`)**: When a validator's direct child PID exits `0` (`waitpid`) and its output stream/screen capture contains a complete answer, mark the call `OutcomeClass::Ok` immediately. Reap any surviving process-group descendants (`-pgid`) asynchronously (`SIGTERM` → `200ms` → `SIGKILL`) without failing the completed call.
- **`FR10` (Embedded Ghostty-VT & 22-Agent Screen Detector — Fixes `D2`)**: Every CLI validator running in Tier 2 or Tier 3 executes inside a `portable-pty` PTY backed by `ghostty-vt` and is continuously evaluated against Herdr's 22 agent `.toml` manifests (`amp`, `agy`, `claude`, `cline`, `codex`, `cursor`, `devin`, `droid`, `gemini`, `github-copilot`, `grok`, `hermes`, `kilo`, `kimi`, `kiro`, `letta`, `maki`, `muse`, `opencode`, `pi`, `qodercli`, `qwen`).
- **`FR11` (Blocker Auto-Remediation Policy)**: When `lope-detect` reports `AgentState::Blocked` on a `AutoRemediateSafe` rule (`startup_update`, or `trust_directory`/`trust_prompt` inside a verified Lope scratch/workspace directory), automatically inject the dismissal/confirm keystroke within `250ms` (max `2` auto-remediations per call) and record a `blocker_remediated` event.
- **`FR12` (Blocker Fail-Fast Policy)**: When `lope-detect` reports `AgentState::Blocked` on a `FailFastImmediate` rule (`permission_prompt`, `live_strong_blocker`, `mcp_elicitation_prompt` during read-only operations, or `auth_error`) and no interactive operator is attached, abort that validator in `< 250ms` with `OutcomeClass::Blocked { rule_id, excerpt }` instead of waiting for the call timeout.
- **`FR13` (Tier 1 Native Async HTTP Engine)**: Execute all `"type": "http"` providers and `switchAILocal` endpoints in-process via `reqwest` (`rustls-tls`) with `0` subprocess spawns.
- **`FR14` (Tier 2 Managed One-Shot PTY Engine)**: Execute stateless CLI validator invocations inside isolated PTYs with bounded head+tail stream capture (`2 MB` stdout / `512 KB` stderr ceiling) and clean ANSI stripping (`ghostty-vt` plain-text extraction).
- **`FR15` (Tier 3 Persistent Warm Agent Pool)**: In `loped`, maintain a configurable pool of warm interactive CLI panes (`Idle` state verified via `lope-detect`), leasing panes to incoming calls and resetting session state between runs.
- **`FR16` (Curl Parser & Team CLI)**: Support `lope team {list,add,remove,test,enable,disable}` and `lope team add <name> --from-curl "<curl-command>"`, parsing URLs, `-H`/`--header`, `-d`/`--data`, and `${ENV_VAR}` placeholders into `~/.lope/config.json`.

### 1.3 Single-Shot, Consensus Review & Role Lenses (`FR17`–`FR24`)
- **`FR17` (Single-Shot Verbs `ask`, `vote`, `compare`, `pipe`)**: Implement parallel fan-out across enabled validators with per-validator error isolation by default and strict failure on `--require-all`.
- **`FR18` (Single-Positional `lope review` Contract)**: Accept one positional `<file>` argument (plus `--consensus [quorum|unanimous|any]`, `--format {text,json,markdown,markdown-pr,sarif}`, `--output <path>`, `--focus <lens>`, `--remember`, `--brain-context <query>`, `--brain-log`, `--include-raw`).
- **`FR19` (Direct Atomic Report File Output `--output <path>`)**: When `--output <path>` is passed to `lope review` or `lope deliberate`, write clean, ANSI-free, redacted UTF-8 output atomically via temp-file rename while streaming live progress to stdout/stderr.
- **`FR20` (7 Glimmer Role Lenses `--focus`)**: Support arbitrary `--focus "<text>"` strings as well as shorthand aliases (`product`, `sales`, `tax`, `finance`, `legal`, `research`, `engineering`) matching Glimmer's 7-role Council selector (`FR38` in Glimmer).
- **`FR21` (Finding Extraction, Deduplication & Quorum Scoring)**: Parse structured `- [SEVERITY] <file>:<line> — <message> (confidence: 0.XX)` bullets and prose fallback, cluster duplicate findings across validators by file + line proximity (`±5` lines) + token similarity, and compute consensus scores.
- **`FR22` (SARIF 2.1.0 Export)**: Emit valid SARIF 2.1.0 JSON when `--format sarif` is passed to `lope review` or `lope check`.
- **`FR23` (Verdict Block Parser & Repair)**: Parse `---VERDICT--- ... ---END---` blocks (JSON-first, YAML/regex fallback) and support `verdict_repair` one-shot structural repair when a model omits delimiters.
- **`FR24` (Caveman & Redaction Filters)**: Apply canonical secret redaction to all outputs and support terse caveman output formatting when configured.

### 1.4 Memory, Makakoo Brain Bridge & Headroom (`FR25`–`FR27`)
- **`FR25` (SQLite Finding Memory `~/.lope/memory.db`)**: Implement `lope memory {stats,search,file,hotspots,forget}` backed by `rusqlite` (`bundled-full`) using the exact table schema of Python `lope/memory.py` (`findings`, `runs`) plus FTS5 virtual table indexing. Honor `LOPE_MEMORY=off`.
- **`FR26` (Makakoo Brain Context `--brain-context <query>`)**: Probe `MAKAKOO_BIN` / `makakoo` on `PATH` and `MAKAKOO_HOME`. Execute `makakoo search <query>` (or read-only SQLite FTS5 fallback), redact secrets, trim to `--brain-budget` (default `1,200` tokens), and prepend the advisory block once.
- **`FR27` (Makakoo Journal Log `--brain-log` & Auto-Sync)**: Append a single Logseq outliner bullet (`- [[Lope]] ...`) to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md` and invoke `makakoo sync` so the Brain FTS index updates immediately.

### 1.5 Council Deliberation, Sprints, Flow Graphs & TUI (`FR28`–`FR40`)
- **`FR28` (7-Stage Council Deliberation `lope deliberate`)**: Implement all 7 stages (Intake → Independent Positions → Anonymized `Response A/B/C` Critique → Revision → Primary Synthesis → Rubric Review → Minority Report) across all 6 templates (`adr`, `prd`, `rfc`, `build-vs-buy`, `migration-plan`, `incident-review`) and 3 depths (`quick`, `standard`, `deep`). Enforce contributor quorum (`expected / 2 + 1`).
- **`FR29` (Multi-Round Sprint Negotiation `lope negotiate`)**: Implement `lope negotiate <goal> [--domain {engineering,business,research}] [--rounds N] [--out <path>]`, writing `SPRINT-<SLUG>.md` with domain-specific phase rubrics.
- **`FR30` (Phase Executor `lope execute`)**: Execute `SPRINT-<SLUG>.md` phases sequentially: primary implements → objective gates run → validator ensemble critiques → retry/heal on `NEEDS_FIX` up to `--max-retries` → advance on `PASS`.
- **`FR31` (Autonomous Swarm Implementation `lope implement`)**: Execute full sprint backlogs with primary + escalation roster, enforcing child-worktree isolation when invoked from Glimmer and bounding recursion via `LOPE_IMPLEMENTATION_DEPTH`.
- **`FR32` (Sprint Audit Scorecard `lope audit` & Gates `lope gate`/`check`)**: Compute per-phase verdicts, confidence, and duration from sprint artifacts and execute shell/file verification gates.
- **`FR33` (Declarative DOT Flow Graph Engine `lope flow`)**: Parse `.dot` graphs (`lope flow {run,validate,render,templates}`), execute wave-based concurrent fan-out and barrier fan-in over `agent`, `review`, `judge`, and `script` nodes, interpolate blackboard state, and enforce per-node `max_visits` and graph-wide `max_node_visits`.
- **`FR34` (Live Ratatui Split-Pane TUI `lope attach`)**: Connect to `loped` and render live validator PTY buffers, status badges (`IDLE`, `WORKING`, `BLOCKED`, `DONE`), and keyboard/mouse pane focus (`Ctrl+B` prefix + mouse clicks).
- **`FR35` (Remote SSH Machine Target `--machine <host>`)**: Allow delegating validator PTY execution to a saved remote SSH host via Herdr's SSH endpoint transport.
- **`FR36` (Host Skill Installer `lope install`)**: Install/update skill definitions across Claude Code, Codex, Cursor, Gemini, OpenCode, and Antigravity (`~/.gemini/skills/lope*`).
- **`FR37` (Built-In Reference Docs `lope docs`)**: Print complete, up-to-date CLI and architecture documentation to stdout.
- **`FR38` (JSON-RPC Socket Protocol `lope.socket.v1`)**: Expose `status`, `review`, `ask`, `deliberate`, `pool.list`, `pool.warm`, `jobs.list`, `jobs.prune`, and `events.subscribe` over `~/.lope/run/lope.sock`.
- **`FR39` (Graceful Signal & Parent-Death Handling)**: On `SIGINT`/`SIGTERM` or client disconnect (when running in ephemeral `C1` mode), cleanly terminate owned child process groups within `500ms`.
- **`FR40` (Zero Orphaned Temp Files)**: Clean up all per-call prompt files (`0600`) and scratch directories (`0700`) via RAII `Drop` guards even on panic or cancellation.

---

## 2. Non-Functional Requirements (`NFR1`–`NFR15`)

- **`NFR1` (Rust Toolchain & Single Binary)**: Compile with stable Rust `2021` into a single self-contained native binary (`lope`) with bundled SQLite (`rusqlite` `bundled-full`) and pure-Rust TLS (`rustls-tls`). Zero runtime dependency on Python.
- **`NFR2` (Intel Mac Resource Budget)**:
  - Cold CLI startup (`lope version`, `lope --help`): **`< 10 ms`** wall-clock time.
  - Idle `loped` daemon RSS (with 0 warm panes): **`< 15 MB`**.
  - Active 5-validator fan-out RSS (excluding external Node/Python CLI children): **`< 30 MB`**.
  - Zero busy-wait polling loops (`0%` CPU when waiting on PTY or HTTP I/O).
- **`NFR3` (Unix Socket Permissions & Peer Auth)**: `~/.lope/run` must be `0700`, `lope.sock` must be `0600`, and every connection must verify `peer_uid == geteuid()` via `LOCAL_PEERCRED` / `getpeereid`.
- **`NFR4` (No Shell Execution for Validators)**: Validator commands must never pass through `/bin/sh -c`. Only explicit `gate`/`script` nodes explicitly authored in a sprint or `.dot` flow may execute shell commands.
- **`NFR5` (Redaction Before Persistence)**: No unredacted secret (`sk-`, `ghp_`, `AKIA`, `-----BEGIN`, configured API keys) may ever be written to disk, SQLite, Brain journals, or stdout.
- **`NFR6` (Manifest Privacy)**: Run manifests in `~/.lope/state/sprints/` must never contain `prompt`, `response`, `argv`, or `env` bodies.
- **`NFR7` (Blocker Detection Latency)**: Screen-state transitions in `ghostty-vt` must be evaluated within **`50 ms`** of PTY output quiescence and trigger blocker policies in **`< 250 ms`**.
- **`NFR8` (Pure State / Runtime Separation)**: Core state machines (`DeliberationRun`, `ConsensusReport`, `FlowGraph`, `SprintExecution`) must be pure Rust structs unit-testable with deterministic mock providers in `< 100 ms`.
- **`NFR9` (Cross-Platform Isolation)**: OS-specific code (`setsid`, `killpg`, `LOCAL_PEERCRED`, Windows Job Objects) must live exclusively in `lope-runtime-pty::platform::<os>`.
- **`NFR10` (SQLite Concurrency Safety)**: `~/.lope/memory.db` must open with `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;` so concurrent Lope runs never fail with `database is locked`.
- **`NFR11` (Atomic File Writes)**: All updates to `~/.lope/config.json`, run manifests, and `--output <path>` reports must write to a `.tmp` sibling with `fsync` and `rename`.
- **`NFR12` (License Compliance)**: Preserve `Apache-2.0` license headers and attribution for Herdr-derived crates (`ghostty-vt`, `lope-detect`, `portable-pty`) and `MIT` compatibility for Lope/Makakoo components.
- **`NFR13` (Evidence Hygiene)**: Sprint evidence files in `docs/evidence/` must never contain secrets or raw `~/.lope/config.json` dumps.
- **`NFR14` (Deterministic Test Fixtures)**: All automated tests (`A1`–`A16`) must run offline using local mock PTY binaries and mock HTTP servers without requiring paid external LLM calls.
- **`NFR15` (Zero Regression on Glimmer `A1`–`A15`)**: Cutover to Super Lope at `/usr/local/bin/lope` must keep all existing Glimmer (`makakoo-jungle`) automated and manual checks green.

---

## 3. Later On Purpose — Explicit v2.0 Non-Goals (`L1`–`L8`)

- **`L1`**: No Electron or web GUI inside Super Lope (`Glimmer.app` is the desktop GUI; `lope attach` is the terminal Ratatui TUI).
- **`L2`**: No local LLM inference engine (no embedded `llama.cpp` or `candle` weights inside the `lope` binary — local models remain external providers via Ollama/llama.cpp HTTP or subprocess adapters).
- **`L3`**: No modification of `Glimmer.app` TypeScript source code required for Super Lope v2.0 compatibility.
- **`L4`**: No automatic git commits or branch pushes on `PASS` verdicts (`human commits` rule remains absolute).
- **`L5`**: No cloud account or SaaS relay required for `loped` or `--machine` SSH connections.
- **`L6`**: No breaking changes to `~/.lope/config.json` or `~/.lope/memory.db` schemas.
- **`L7`**: No auto-approval of destructive tool permissions (`rm`, `git push`, file writes) when a validator runs in read-only `review`, `ask`, `vote`, `compare`, or `deliberate` modes.
- **`L8`**: No Python interpreter embedded via `PyO3` — 100% pure Rust implementation.

---

## 4. Automated Acceptance Tests (`A1`–`A16`)

| Test ID | Verification Target | Pass Criteria | Sprint |
|---|---|---|---|
| **`A1`** | **CLI Grammar & Version Parity** | All 23 legacy verbs + 4 new verbs parse cleanly; `env -i PATH="/usr/bin:/bin:/usr/local/bin" lope version` and `lope --version` exit `0` in `< 10ms`. | [`S0`](../sprints/S0-workspace-and-compat-cli.md) |
| **`A2`** | **Config & Verdict Parser Compatibility** | Existing `~/.lope/config.json` loads without loss; JSON and YAML-ish `---VERDICT---...---END---` blocks and `verdict_repair` prompts parse identically to Python Lope. | [`S0`](../sprints/S0-workspace-and-compat-cli.md) |
| **`A3`** | **Darwin Descendant-Safe Reaping (`Fixes D1 & D5`)** | Mock validator prints valid verdict, forks a child holding stdout/stderr open and sleeping `30s`, and exits `0`. `lope-runtime-pty` returns `Ok(verdict)` in `< 300ms` and reaps the child group within `1s`. | [`S1`](../sprints/S1-pty-supervisor-and-detector.md) |
| **`A4`** | **22-Agent Screen Manifest Detection (`Fixes D2`)** | Replay captured terminal buffers for `claude`, `codex`, `gemini`, `agy`, `grok`, `opencode`, `kimi`, and `pi` through `ghostty-vt` + `lope-detect`. 100% of `Idle`, `Working`, and `Blocked` fixtures match expected rule IDs. | [`S1`](../sprints/S1-pty-supervisor-and-detector.md) |
| **`A5`** | **Blocker Auto-Remediation & Fail-Fast** | Mock PTY agent emitting `startup_update` and `trust_directory` (in scratch dir) is auto-remediated in `< 250ms` and succeeds; mock agent emitting `live_blocked_form` during `review` fails fast in `< 250ms` with `OutcomeClass::Blocked`. | [`S1`](../sprints/S1-pty-supervisor-and-detector.md) |
| **`A6`** | **Daemon Socket Auth & Warm Pool Reuse** | `loped` binds `~/.lope/run/lope.sock` (`0600`), verifies `LOCAL_PEERCRED`, warms 2 mock interactive panes, and completes 4 sequential calls using the warm panes with zero extra process spawns. | [`S2`](../sprints/S2-warm-pool-and-daemon.md) |
| **`A7`** | **Tier 1 Native HTTP & `--from-curl`** | `lope team add` with `--from-curl` parses headers/body/env vars accurately; `reqwest` adapter fans out to local axum mock server with `0` child processes spawned. | [`S3`](../sprints/S3-validators-and-http-engine.md) |
| **`A8`** | **Budget Leases, Retries & Redaction** | Exceeding `max_calls` or `remaining_seconds` raises `BudgetExhausted`; `rate_limited` retries once; secret patterns (`sk-proj-...`, `ghp_...`, `AKIA...`) are 100% scrubbed from outputs and error diagnostics. | [`S3`](../sprints/S3-validators-and-http-engine.md) |
| **`A9`** | **Single-Shot Verbs (`ask`, `vote`, `compare`, `pipe`)** | Fan-out across 3 mock validators tallies votes and A/B comparisons accurately; 1 failing validator does not abort default run, and fails run when `--require-all` is set. | [`S4`](../sprints/S4-single-shot-and-consensus-review.md) |
| **`A10`** | **Consensus Review, Role Lenses, `--output` & SARIF** | `lope review <file> --consensus quorum --format markdown --focus legal --output <out.md>` deduplicates overlapping findings across 3 validators, writes clean Markdown to `<out.md>`, and `--format sarif` validates against SARIF 2.1.0 schema. | [`S4`](../sprints/S4-single-shot-and-consensus-review.md) |
| **`A11`** | **SQLite Finding Memory & WAL Concurrency** | `lope review --remember` inserts findings into `~/.lope/memory.db`; `lope memory {stats,search,file,hotspots,forget}` returns accurate results; 4 concurrent writers complete with zero SQLite lock errors. | [`S5`](../sprints/S5-memory-and-makakoo-bridge.md) |
| **`A12`** | **Makakoo Brain Bridge (`--brain-context` & `--brain-log`)** | `--brain-context` queries `makakoo search`, redacts, and prepends bounded context once; `--brain-log` appends `- [[Lope]] ...` to today's journal and invokes `makakoo sync`. | [`S5`](../sprints/S5-memory-and-makakoo-bridge.md) |
| **`A13`** | **7-Stage Council Deliberation & Quorum Gate** | `lope deliberate` executes all 7 stages across `adr` and `prd` templates, strips validator names to `Response A/B/C` during critique/revision, preserves high-severity `NEEDS_FIX` objections in Minority Report, and fails with `DeliberationInconclusive` if quorum `< N/2 + 1`. | [`S6`](../sprints/S6-council-deliberation.md) |
| **`A14`** | **Sprint Negotiation, Execution & Child-Worktree Implement** | `lope negotiate --domain business` writes `SPRINT-<SLUG>.md`; `lope implement` runs phase implementation + gate + validator retry loop (`NEEDS_FIX` → `PASS`); `lope audit` generates accurate scorecard. | [`S7`](../sprints/S7-sprints-and-flow-graphs.md) |
| **`A15`** | **Declarative DOT Flow Engine & Visit Caps** | `lope flow validate` catches malformed graphs; `lope flow run` executes parallel wave fan-out, barrier fan-in, conditional routing, and stops cyclical graphs at `max_node_visits`. | [`S7`](../sprints/S7-sprints-and-flow-graphs.md) |
| **`A16`** | **Glimmer End-to-End Integration & Benchmark Gate** | With `/usr/local/bin/lope` switched to Super Lope, `glimmer doctor`, `lope jobs prune --stale`, and Glimmer's companion socket suite pass 100%, with startup `< 10ms` and RSS `< 25 MB`. | [`S8`](../sprints/S8-glimmer-cutover-and-benchmarks.md) |

---

## 5. Manual Acceptance Tests (`M1`–`M8`)

- **`M1` (Live Multi-CLI `lope ask` & `lope review` on Intel Mac)**: Run a live `lope review README.md --consensus quorum --format markdown` across the real validator team (`codex`, `claude`, `grok`, `agy`, `kimi`) and verify zero false `cleanup_failed` errors and full consensus report output.
- **`M2` (Live Blocker Interception)**: Trigger a CLI validator in an untrusted temp folder and observe `lope` auto-remediating or reporting `Blocked` in `< 250ms` instead of hanging for 15 minutes.
- **`M3` (Warm Pool Speedup)**: Start `lope daemon start`, run `lope pool warm`, and compare wall-clock latency of two consecutive `lope ask` queries.
- **`M4` (Live `lope attach` TUI)**: Start a `lope deliberate` run and run `lope attach` in a second terminal tab to watch the live split-pane PTYs and status badges, then detach with `Ctrl+B q`.
- **`M5` (Glimmer Desktop Watched Council)**: Open `Glimmer.app`, run **Council these notes** with a Business/Tax lens (`--focus`), verify the watched terminal runs Super Lope cleanly, and confirm the report opens quietly in Glimmer.
- **`M6` (Glimmer `Cmd-Q` Quit Survival)**: Start a Council run in Glimmer, press `Cmd-Q`, and verify Super Lope finishes writing `.lope/reports/council-<id>.md`.
- **`M7` (Makakoo Journal & Brain Search)**: Run `lope review <file> --brain-context "Makakoo" --brain-log` and verify `makakoo search "Lope"` immediately retrieves the new journal entry.
- **`M8` (Rollback & Upgrade Drill)**: Verify `lope-py version` still runs the legacy Python fallback if invoked explicitly, while `lope version` runs the native Rust binary.

---

[Specs Index](README.md) · Previous: [02 — Use Cases & Scenarios](02-use-cases-and-scenarios.md) · Next: [04 — Interfaces & Protocol](04-interfaces-and-protocol.md)
