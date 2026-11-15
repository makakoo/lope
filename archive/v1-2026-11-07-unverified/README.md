# Super Lope (`super-lope`) — Rust Multi-Agent Ensemble & PTY Runtime

**Super Lope** (`lope` v2.0, binary name `lope`, daemon `loped` / `com.makakoo.lope`) is the clean-room **Rust** fusion of **Lope** (`0.17.3` multi-CLI validator ensemble, sprint engine, 7-stage Council deliberation, and DOT flow runner) and **Herdr** (`0.9.5` Rust terminal workspace multiplexer, `ghostty-vt` virtual terminal emulator, and 22-agent screen-state detection engine), purpose-built for **Makakoo OS** (`0.4.0+`, persona **Harvey**) and **Glimmer** (`makakoo-jungle`).

- **CLI Binary**: `/usr/local/bin/lope` (100% drop-in CLI, flag, exit-code, and file-format compatible with Python Lope `0.17.3+`)
- **Background Daemon**: `com.makakoo.lope` (`~/.lope/run/lope.sock`, plus optional live TUI attach via `lope attach`)
- **State & Config Root**: `~/.lope/` (`config.json`, `memory.db`, `state/sprints/`)
- **Specification & Plan Repository**: `/Users/sebastian/Projects/apps/super-lope`
- **Upstream Reference Repositories**:
  - Python Lope (`0.17.3` @ `9cffa55`): `/Users/sebastian/.lope` (`61` Python files, `27,322` LOC)
  - Rust Herdr (`0.9.5` @ `ee5bfb50`): `/Users/sebastian/projects/apps/herdr` (`291,081` Rust LOC in `src/` + `crates/ghostty-vt`)
  - Makakoo OS (`0.4.0` @ `2e1bfe7`): `/Users/sebastian/projects/apps/makakoo-os` (`6` Cargo workspace crates)
  - Glimmer (`makakoo-jungle` @ `8a541055`): `/Users/sebastian/Projects/apps/makakoo-jungle`

---

## Why Super Lope Exists (Root-Cause Summary)

Live source audits of `/Users/sebastian/.lope` and `/Users/sebastian/projects/apps/herdr` identified the exact mechanical reasons why Python Lope experiences frequent 120s–900s timeouts, high CPU/RAM pressure on the dogfood Intel Mac (`i5-8257U` / 16 GB RAM), and `21` abandoned `cleanup_failed` manifests:

1. **The Darwin Process-Group Reaping Defect (`lope/supervisor.py:102-118`, `476-495`)**:
   - On macOS (`sys.platform == "darwin"`), `_process_group_count(pgid)` skips `ps` and returns `1` whenever `os.killpg(pgid, 0)` succeeds. When a validator CLI (`claude`, `codex`, `grok`, `agy`, `opencode`) exits after printing a complete answer on `stdout` (`stdout_complete=True`), any background helper or MCP server still alive in its process group after `0.7s` (`CLEANUP_TIMEOUT_SECONDS`) causes `_terminate_and_reap` to return `"cleanup_failed:descendants_live"`. `lope/processes.py:314` then raises `ProcessCompletionError` and **discards the valid completed response**.
2. **Blind Pipe Deadlocks on Interactive TUI Prompts (`lope/supervisor.py:355-368` vs. `herdr/src/detect/manifests/*.toml`)**:
   - Python Lope spawns CLIs over raw `subprocess.PIPE` (`stdin` closed, no PTY). When `codex`, `claude`, or `agy` runs in a new worktree or temp directory and renders an interactive blocker — such as a **Folder Trust dialog** (`"Do you trust the contents of this directory?"`), **Startup Update picker** (`"Update available!"`), or **Tool/MCP Permission prompt** — Python Lope cannot see the terminal screen and sits blind until the `900s` wall-clock timeout expires.
3. **Double-Interpreter Subprocess Churn & `20ms` Busy-Wait Polling (`lope/processes.py:231-270`)**:
   - Every single model call spawns a second Python interpreter (`python3 -m lope.supervisor <result_path>`), which in turn cold-boots a heavy Node.js/Python CLI, while the parent thread spins in a `time.sleep(0.02)` loop (`50 Hz` per concurrent validator) and flushes full JSON manifests to disk on every state transition.

**Super Lope eliminates all three defects at the architectural root** by replacing Python's double-interpreter pipe supervisor with Herdr's native Rust `tokio` + `portable-pty` + `ghostty-vt` virtual terminal engine, real-time 22-agent screen-state detector (`Idle`, `Working`, `Blocked`, `Unknown`), and persistent warm agent pane pools.

---

## Documentation Architecture

This repository is organized into three backlinked documentation pillars plus the master executable sprint document:

```text
/Users/sebastian/Projects/apps/super-lope/
├── README.md                                              # Master project index (this file)
├── SPRINT-SUPER-LOPE-V1.md                                # Final executable master sprint document (Phases 1–9 / S0–S8)
└── docs/
    ├── audits/                                            # 1. Ground-truth live system audits
    │   ├── README.md                                      #    Audits index, live pin table & root-cause defect matrix
    │   ├── 01-lope-python-audit.md                        #    Python Lope 0.17.3 (61 files, 27,322 LOC) architecture & failure audit
    │   ├── 02-herdr-rust-audit.md                         #    Herdr 0.9.5 (291,081 LOC) PTY, ghostty-vt, detect manifests & server audit
    │   └── 03-makakoo-and-glimmer-seams-audit.md          #    Makakoo OS 0.4.0 & Glimmer (makakoo-jungle) CLI/socket compatibility audit
    ├── specs/                                             # 2. Canonical product & engineering specifications
    │   ├── README.md                                      #    Specification index & canonical ID registry
    │   ├── 01-product-and-architecture.md                 #    Crate workspace, 3-tier execution engine, state machines & security
    │   ├── 02-use-cases-and-scenarios.md                  #    Use cases (UC1–UC18), scenarios (SC1–SC15) & operator journeys
    │   ├── 03-requirements-and-acceptance.md              #    FR1–FR40, NFR1–NFR15, L1–L8, tests A1–A16 & M1–M8
    │   └── 04-interfaces-and-protocol.md                  #    Drop-in CLI argv contract, lope.socket.v1, SQLite schemas, DOT flow & SARIF
    └── sprints/                                           # 3. Serial sprint execution plan (S0–S8)
        ├── README.md                                      #    Master sprint roadmap, sequence & traceability matrix
        ├── S0-workspace-and-compat-cli.md                 #    S0: Cargo workspace bootstrap, ghostty-vt & 100% CLI/config parser parity
        ├── S1-pty-supervisor-and-detector.md              #    S1: PTY actor, Herdr 22-agent screen detector & Darwin reaping fix
        ├── S2-warm-pool-and-daemon.md                     #    S2: Background daemon (lope.sock), warm agent pool & blocker remediation
        ├── S3-validators-and-http-engine.md               #    S3: Native Tokio HTTP adapter, 15 CLI adapters, curl parser & team CLI
        ├── S4-single-shot-and-consensus-review.md         #    S4: ask/vote/compare/pipe + consensus review, lenses (--focus), SARIF & --output
        ├── S5-memory-and-makakoo-bridge.md                #    S5: Rusqlite FTS5 finding memory, Makakoo Brain bridge & Headroom MCP
        ├── S6-council-deliberation.md                     #    S6: 7-stage adversarial Council deliberation + live Herdr TUI view
        ├── S7-sprints-and-flow-graphs.md                  #    S7: Sprint engine (negotiate/implement/execute/audit) & DOT flow graphs
        └── S8-glimmer-cutover-and-benchmarks.md           #    S8: Glimmer drop-in cutover, Intel Mac scale benchmarks & release binary
```

---

## Quick Navigation

### 1. Audits (`docs/audits/`)
Verified ground-truth facts from the live dogfood Mac:
- **[Audits Index, Live Pin Table & Defect Matrix](docs/audits/README.md)**
- **[01 — Python Lope Audit (`0.17.3`)](docs/audits/01-lope-python-audit.md)**: Module-by-module inventory (`61` files, `27,322` LOC), all `21` CLI verbs, `supervisor.py` Darwin process-group bug, blind pipe deadlocks, `config.json` provider schema, SQLite `memory.db`, and `~/.lope/state/sprints/` lease lifecycle.
- **[02 — Rust Herdr Audit (`0.9.5`)](docs/audits/02-herdr-rust-audit.md)**: Module inventory (`291,081` LOC + `crates/ghostty-vt`), `PtyIoActorHandle` (`src/pty/actor.rs`), `AgentDetection` & `22` `.toml` screen manifests (`src/detect/`), headless daemon (`src/server/headless.rs`), `wait_for_output` (`src/api/wait.rs`), and SSH multi-machine endpoints.
- **[03 — Makakoo OS & Glimmer Seams Audit](docs/audits/03-makakoo-and-glimmer-seams-audit.md)**: Exact CLI flags, stdout formats, exit codes, and filesystem paths expected by `makakoo` (`0.4.0`), `glimmer` (`makakoo-jungle`), and the 12 installed Lope skills (`lope`, `lope-review`, `lope-deliberate`, `lope-flow`, etc.).

### 2. Specifications (`docs/specs/`)
Single-source-of-truth product and engineering contract:
- **[Specifications Index & Canonical ID Registry](docs/specs/README.md)**
- **[01 — Product & Architecture](docs/specs/01-product-and-architecture.md)**: Cargo workspace topology (`lope-cli`, `lope-core`, `lope-runtime-pty`, `lope-detect`, `lope-providers`, `lope-engine`, `lope-memory`, `lope-flow`, `lope-tui`, `ghostty-vt`), the 3-Tier Execution Engine (`Tier 1: Native HTTP`, `Tier 2: Managed One-Shot PTY`, `Tier 3: Persistent Warm Agent Pool`), Blocker Policy Engine, and Security boundaries.
- **[02 — Use Cases & Scenarios](docs/specs/02-use-cases-and-scenarios.md)**: All 18 Use Cases (`UC1`–`UC18`), 15 Verification Scenarios (`SC1`–`SC15`), and Operator Journeys (CLI, Glimmer Desktop, Headless Daemon, and Interactive Live Council TUI).
- **[03 — Requirements & Acceptance](docs/specs/03-requirements-and-acceptance.md)**: Functional Requirements (`FR1`–`FR40`), Non-Functional Requirements (`NFR1`–`NFR15`), Explicit Non-Goals (`L1`–`L8`), Automated Tests (`A1`–`A16`), and Manual Tests (`M1`–`M8`).
- **[04 — Interfaces & Protocol](docs/specs/04-interfaces-and-protocol.md)**: Complete `lope` CLI `argv` & exit-code specification, `lope.socket.v1` JSON-RPC daemon protocol, `~/.lope/config.json` & `lope.toml` schemas, `---VERDICT---` & consensus parsers, SQLite `memory.db` schema, DOT flow grammar, and SARIF 2.1.0 schema.

### 3. Sprints (`SPRINT-SUPER-LOPE-V1.md` & `docs/sprints/`)
Strictly serial implementation slices (`S0` → `S8`), gated by `docs/evidence/` outputs:
- **[Master Executable Sprint (`SPRINT-SUPER-LOPE-V1.md`)](SPRINT-SUPER-LOPE-V1.md)**
- **[Sprint Roadmap & Traceability Matrix](docs/sprints/README.md)**
- **[S0 — Workspace & Compat CLI](docs/sprints/S0-workspace-and-compat-cli.md)** → **[S1 — PTY Supervisor & Detector](docs/sprints/S1-pty-supervisor-and-detector.md)** → **[S2 — Warm Pool & Daemon](docs/sprints/S2-warm-pool-and-daemon.md)** → **[S3 — Validators & HTTP Engine](docs/sprints/S3-validators-and-http-engine.md)** → **[S4 — Single-Shot & Consensus Review](docs/sprints/S4-single-shot-and-consensus-review.md)** → **[S5 — Memory & Makakoo Bridge](docs/sprints/S5-memory-and-makakoo-bridge.md)** → **[S6 — Council Deliberation](docs/sprints/S6-council-deliberation.md)** → **[S7 — Sprints & Flow Graphs](docs/sprints/S7-sprints-and-flow-graphs.md)** → **[S8 — Glimmer Cutover & Benchmarks](docs/sprints/S8-glimmer-cutover-and-benchmarks.md)**

---

## Core Architectural Invariants

1. **100% Drop-In Backward Compatibility (`FR1`–`FR4`)**: The compiled Rust `lope` binary accepts every existing subcommand (`negotiate`, `execute`, `implement`, `audit`, `ask`, `review`, `vote`, `compare`, `pipe`, `team`, `deliberate`, `memory`, `gate`, `check`, `status`, `jobs`, `configure`, `install`, `update`, `upgrade`, `version`, `docs`, `flow`), every flag expected by Glimmer (`--consensus`, `--format`, `--output`, `--focus`, `--brain-context`, `--brain-log`, `--remember`, `--domain`, `jobs prune --stale`), and reads existing `~/.lope/config.json` and `~/.lope/memory.db` without migration friction.
2. **Fast-PathBlocker Detection, Never Blind Waiting (`FR10`–`FR12`)**: Every CLI validator executing in a PTY is continuously monitored by the Herdr screen-state detector (`ghostty-vt` + `22` agent `.toml` manifests). If an agent transitions to `AgentState::Blocked` (e.g., trust prompt, update dialog, permission prompt, auth error), Super Lope reacts in **`< 250ms`** — applying deterministic safe auto-remediation (e.g., dismissing startup update banners or confirming read-only folder trust in isolated scratch dirs) or failing fast with `OutcomeClass::Blocked(rule_id)` instead of hanging for `900s`.
3. **Leader-Completion Priority on Darwin (`FR9`)**: When a validator leader process exits `0` and `stdout` / terminal capture contains a complete response, background descendant helpers (MCP servers, language servers) left in the process group never invalidate or discard the completed response. Descendants are cleaned up asynchronously with `SIGTERM` → `SIGKILL` without failing the completed call.
4. **3-Tier Execution Routing (`FR13`–`FR15`)**:
   - **Tier 1 (Native Async HTTP)**: OpenAI-compatible gateways (`switchAILocal`, pods, cloud APIs, `--from-curl` providers) run directly on `tokio` + `reqwest` connection pools with zero subprocesses.
   - **Tier 2 (Managed One-Shot PTY)**: Stateless CLI flags (`claude --print`, `codex exec`, `gemini --prompt`) run inside bounded `portable-pty` + `ghostty-vt` actors with real-time blocker detection.
   - **Tier 3 (Warm Persistent Agent Pool)**: Interactive CLI sessions stay warm inside the `loped` background daemon (`~/.lope/run/lope.sock`), eliminating the 1–3s Node.js/Python cold-boot tax across multi-round reviews, deliberations, and sprints.
5. **Single Self-Contained Rust Binary (`NFR1`–`NFR3`)**: Compiles to a single static binary (`/usr/local/bin/lope`) with `< 15 MB` idle RSS, `< 10 ms` CLI startup latency, and zero runtime dependence on `/usr/bin/python3` or `PYTHONPATH`.
