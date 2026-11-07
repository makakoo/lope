# SPRINT-SUPER-LOPE-V1

[Root Index](README.md) · [Audits](docs/audits/README.md) · [Specs](docs/specs/README.md) · [Sprint Roadmap](docs/sprints/README.md)

## Origin

Deliver **Super Lope v2.0** (`/Users/sebastian/Projects/apps/super-lope`, binary `/usr/local/bin/lope`, daemon `loped` / `com.makakoo.lope`): a clean-room **Rust** fusion of **Lope (`0.17.3`)** and **Herdr (`0.9.5`)** built on the **Makakoo OS (`0.4.0+`)** Rust stack. Super Lope replaces Python Lope's double-interpreter pipe supervisor with Herdr's `portable-pty` + `ghostty-vt` virtual terminal engine, 22-agent screen-state detector, and persistent warm agent pool — permanently eliminating Darwin `cleanup_failed` false failures (`D1`), 15-minute blind interactive prompt hangs (`D2`), and multi-interpreter CPU/RAM thrashing (`D3`), while maintaining 100% drop-in CLI, config, and socket compatibility with **Glimmer (`makakoo-jungle`)** and **Makakoo OS**.

---

## Non-Negotiable Product Invariants

1. **100% Drop-In Compatibility with Glimmer & Makakoo OS ([`FR1`–`FR4`](docs/specs/03-requirements-and-acceptance.md#1-functional-requirements-fr1fr40))**:
   - Accepts all 23 existing Lope subcommands (`negotiate`, `execute`, `implement`, `audit`, `ask`, `review`, `vote`, `compare`, `pipe`, `team`, `deliberate`, `memory`, `gate`, `check`, `status`, `jobs`, `configure`, `install`, `update`, `upgrade`, `version`, `docs`, `flow`) plus 4 new runtime subcommands (`daemon`, `pool`, `attach`, `detect`).
   - Supports `lope version`, `lope --version`, and `lope -V`, outputting a version token `>= 0.17.3` (`lope 2.0.0 (super-lope; compat 0.17.3)`) that satisfies `glimmer doctor`.
   - Preserves existing `~/.lope/config.json`, `~/.lope/memory.db`, and `~/.lope/state/sprints/` schemas without migration friction, and keeps `/usr/local/bin/lope-py` as an instant rollback shim.
2. **Leader-Completion Authority on macOS/POSIX ([`FR9`](docs/specs/03-requirements-and-acceptance.md#1-functional-requirements-fr1fr40) — Fixes `D1` & `D5`)**:
   - When a validator's direct child PID exits `0` (`waitpid`) and its output stream/screen capture contains a complete response, the call is immediately recorded as `OutcomeClass::Ok`.
   - Any surviving process-group descendants (`-pgid`, such as background MCP servers or git helpers) are reaped asynchronously (`SIGTERM` → `200ms` → `SIGKILL`) on a background Tokio task and **never** cause a completed model response to be discarded as `cleanup_failed`.
3. **Real-Time PTY Screen Detection & `< 250ms` Blocker Policy ([`FR10`–`FR12`](docs/specs/03-requirements-and-acceptance.md#1-functional-requirements-fr1fr40) — Fixes `D2`)**:
   - Every CLI validator runs inside a `portable-pty` pseudo-terminal backed by `ghostty-vt` and Herdr's 22 agent `.toml` manifests (`claude`, `codex`, `gemini`, `agy`, `grok`, `opencode`, `kimi`, `qwen`, `pi`, etc.).
   - When `AgentState::Blocked` is detected (`visible_blocker == true`), the Blocker Policy Engine reacts in `< 250ms`: auto-remediating safe startup/trust prompts (`startup_update`, or `trust_directory` in verified Lope scratch/workspace dirs) or failing fast with `OutcomeClass::Blocked(rule_id)` when a read-only call hits a tool-permission or auth modal.
4. **3-Tier Execution Routing ([`FR13`–`FR15`](docs/specs/03-requirements-and-acceptance.md#1-functional-requirements-fr1fr40) — Fixes `D3`)**:
   - **Tier 1 (`Native HTTP`)**: Direct `reqwest` (`rustls-tls`) async calls for `"type": "http"` providers and `switchAILocal` (`0` subprocesses).
   - **Tier 2 (`Managed One-Shot PTY`)**: Isolated PTY execution with `ghostty-vt` buffering and `< 250ms` blocker detection when running headlessly without a warm pool.
   - **Tier 3 (`Warm Agent Pool`)**: Pre-warmed interactive agent panes inside `loped` (`~/.lope/run/lope.sock`), eliminating Node.js/Python cold-boot latency across multi-round workflows.
5. **Strictly Serial Execution (`S0` → `S8`)**:
   - Each phase starts only after the previous phase's evidence file exists under `docs/evidence/`.

---

## Pre-Flight Gate (`Before S0`)

Before executing `Phase 1 (S0)`, verify the live pins from [`docs/audits/README.md`](docs/audits/README.md) and write `docs/evidence/pins.txt`:
- Python Lope reference at `/Users/sebastian/.lope` (`0.17.3` @ `9cffa55`, `61` `.py` files, `27,322` LOC).
- Rust Herdr reference at `/Users/sebastian/projects/apps/herdr` (`0.9.5` @ `ee5bfb50`, `291,081` Rust LOC + `crates/ghostty-vt` + `22` `.toml` manifests).
- Makakoo OS `0.4.0` at `/Users/sebastian/projects/apps/makakoo-os` & `/Users/sebastian/MAKAKOO` (persona `Harvey`).
- Glimmer (`makakoo-jungle` @ `8a541055`) at `/Users/sebastian/Projects/apps/makakoo-jungle` (`/usr/local/bin/glimmer`).

---

## Phases

### Phase 1: s0-workspace-and-compat-cli

**Goal:** Initialize the 10-crate Rust `2021` Cargo workspace in `/Users/sebastian/Projects/apps/super-lope`, extract `crates/ghostty-vt` and `vendor/portable-pty` from Herdr, implement `lope-core` (models, `config.json` + `config.toml` loader, secret redaction, `---VERDICT---` parser/repair), and implement the 27-subcommand `clap` CLI grammar in `lope-cli` ([`S0`](docs/sprints/S0-workspace-and-compat-cli.md)).

**Criteria:**
- `docs/evidence/pins.txt` exists before `S0` starts.
- `cargo check --workspace` and `cargo test --workspace` compile and pass cleanly across all 10 crates (`ghostty-vt`, `lope-core`, `lope-detect`, `lope-runtime-pty`, `lope-providers`, `lope-memory`, `lope-engine`, `lope-flow`, `lope-tui`, `lope-cli`).
- `lope version`, `lope --version`, and `lope -V` exit `0` in `< 10ms` and print `lope 2.0.0 (super-lope; compat 0.17.3)` (`FR2`, `A1`).
- `lope-core` loads `/Users/sebastian/.lope/config.json` without losing any provider or setting and parses JSON and YAML-ish `---VERDICT---...---END---` blocks identically to Python Lope (`FR3`, `FR23`, `A2`).
- `docs/evidence/s0-workspace.txt` records test results and startup timing.

**Files:**
- `Cargo.toml`
- `crates/ghostty-vt/`
- `crates/lope-core/`
- `crates/lope-cli/`
- `docs/evidence/pins.txt`
- `docs/evidence/s0-workspace.txt`

**Tests:**
- `A1: cargo test -p lope-cli && time ./target/debug/lope --version`
- `A2: cargo test -p lope-core`

---

### Phase 2: s1-pty-supervisor-and-detector

**Goal:** Implement `lope-detect` (Herdr's screen detector + all 22 agent `.toml` manifests + Blocker Policy Engine) and `lope-runtime-pty` (async PTY actor, `ghostty-vt` buffer integration, and Leader-Completion Darwin process-group reaper) ([`S1`](docs/sprints/S1-pty-supervisor-and-detector.md)).

**Criteria:**
- Starts only after `docs/evidence/s0-workspace.txt` exists on disk.
- `lope-detect` embeds all 22 `.toml` manifests from Herdr (`amp`, `agy`, `claude`, `cline`, `codex`, `cursor`, `devin`, `droid`, `gemini`, `github-copilot`, `grok`, `hermes`, `kilo`, `kimi`, `kiro`, `letta`, `maki`, `muse`, `opencode`, `pi`, `qodercli`, `qwen`) and exposes `lope detect <file> [--agent <id>]` (`FR10`, `A4`).
- `lope-runtime-pty` implements Leader-Completion Authority (`FR9`, `A3`): when the direct child PID exits `0` with complete output, the call succeeds immediately in `< 300ms` even if a child process holds stdout/stderr open or stays alive in the process group; lingering descendants are reaped asynchronously.
- Blocker Policy Engine (`FR11`, `FR12`, `A5`) auto-remediates `startup_update` and verified-workspace `trust_directory` prompts in `< 250ms` and fails fast in `< 250ms` on read-only permission/auth blockers.
- `docs/evidence/s1-pty-detector.txt` is written on disk.

**Files:**
- `crates/lope-detect/`
- `crates/lope-runtime-pty/`
- `docs/evidence/s1-pty-detector.txt`

**Tests:**
- `A3: cargo test -p lope-runtime-pty --test darwin_descendant_reap`
- `A4: cargo test -p lope-detect --test manifest_classification`
- `A5: cargo test -p lope-runtime-pty --test blocker_policy`

---

### Phase 3: s2-warm-pool-and-daemon

**Goal:** Build the privacy-safe run manifest registry (`~/.lope/state/sprints/` + `lope jobs prune --stale`), the `loped` background daemon (`lope.socket.v1` at `~/.lope/run/lope.sock`), and the Tier 3 Persistent Warm Agent Pool (`lope pool`) ([`S2`](docs/sprints/S2-warm-pool-and-daemon.md)).

**Criteria:**
- Starts only after `docs/evidence/s1-pty-detector.txt` exists on disk.
- `lope status` and `lope jobs {list,show,prune --stale,reconcile}` manage `~/.lope/state/sprints/`, enforcing `_FORBIDDEN_MANIFEST_KEYS` and safely pruning dead-PID manifests while leaving live runs untouched (`FR6`, `NFR6`, `SC14`).
- `lope daemon start` binds `~/.lope/run/lope.sock` (`0600` in `0700` dir), verifies `peer_uid == geteuid()` via `LOCAL_PEERCRED`, and idles at `< 15 MB` RSS (`FR5`, `FR38`, `NFR2`, `NFR3`).
- `lope pool {list,warm,drain,reset}` manages warm interactive PTY panes in `loped` and dispatches consecutive calls with `< 50ms` local dispatch overhead (`FR15`, `A6`, `SC5`).
- `docs/evidence/s2-daemon-pool.txt` is written on disk.

**Files:**
- `crates/lope-core/src/jobs.rs`
- `crates/lope-tui/src/daemon.rs`
- `crates/lope-tui/src/pool.rs`
- `docs/evidence/s2-daemon-pool.txt`

**Tests:**
- `A6 & SC5: cargo test -p lope-tui --test daemon_warm_pool`
- `SC14: cargo test -p lope-core --test stale_manifest_prune`

---

### Phase 4: s3-validators-and-http-engine

**Goal:** Implement `lope-providers`: Tier 1 Native Async HTTP validator (`reqwest` + `rustls-tls`), 15 built-in CLI adapters + generic subprocess/HTTP config providers, `--from-curl` parser, `lope team` / `lope configure` CLIs, and budget/lease/retry orchestration ([`S3`](docs/sprints/S3-validators-and-http-engine.md)).

**Criteria:**
- Starts only after `docs/evidence/s2-daemon-pool.txt` exists on disk.
- Tier 1 HTTP providers (`"type": "http"` and `switchAILocal`) execute in-process via `reqwest` with `0` subprocess spawns (`FR13`, `A7`).
- `lope team {list,add,remove,test,enable,disable}` and `lope team add <name> --from-curl "<cmd>"` atomically update `~/.lope/config.json` (`FR16`, `SC6`).
- Admission planner, budget leases (`max_calls`, `max_chunks`, `max_input_bytes`, `remaining_seconds`), single transient retry (`rate_limited`, `service_unavailable`), and canonical secret redaction pass all tests (`FR7`, `FR8`, `NFR5`, `A8`).
- `docs/evidence/s3-validators.txt` is written on disk.

**Files:**
- `crates/lope-providers/`
- `crates/lope-core/src/invocation.rs`
- `crates/lope-core/src/retry_policy.rs`
- `docs/evidence/s3-validators.txt`

**Tests:**
- `A7 & SC6: cargo test -p lope-providers --test http_and_curl_parser`
- `A8: cargo test -p lope-core --test budget_retry_redaction`

---

### Phase 5: s4-single-shot-and-consensus-review

**Goal:** Implement single-shot fan-out verbs (`ask`, `vote`, `compare`, `pipe`) and the consensus review pipeline (`lope review`, finding extraction, fuzzy deduplication, quorum scoring, 7 Glimmer Role Lenses `--focus`, atomic `--output <path>`, and SARIF 2.1.0 export) ([`S4`](docs/sprints/S4-single-shot-and-consensus-review.md)).

**Criteria:**
- Starts only after `docs/evidence/s3-validators.txt` exists on disk.
- `lope ask`, `lope vote`, `lope compare`, and `lope pipe` fan out concurrently with per-validator isolation by default and strict failure on `--require-all` (`FR17`, `A9`).
- `lope review <file>` supports `--consensus [quorum|unanimous|any]`, `--format {text,json,markdown,markdown-pr,sarif}`, `--output <path>`, and all 7 Glimmer Role Lenses via `--focus` (`FR18`–`FR22`, `A10`, `SC7`).
- `docs/evidence/s4-review.txt` is written on disk.

**Files:**
- `crates/lope-engine/src/single_shot.rs`
- `crates/lope-engine/src/review.rs`
- `crates/lope-engine/src/findings.rs`
- `crates/lope-memory/src/sarif.rs`
- `docs/evidence/s4-review.txt`

**Tests:**
- `A9: cargo test -p lope-engine --test single_shot_verbs`
- `A10 & SC7: cargo test -p lope-engine --test consensus_review_and_sarif`

---

### Phase 6: s5-memory-and-makakoo-bridge

**Goal:** Implement `lope-memory`: Rusqlite WAL + FTS5 finding store (`~/.lope/memory.db`), `lope memory {stats,search,file,hotspots,forget}`, and the Makakoo OS Brain bridge (`--brain-context <query>`, `--brain-log` + `makakoo sync`, `LOPE_BRAIN_AUTOMEMORY=1`) ([`S5`](docs/sprints/S5-memory-and-makakoo-bridge.md)).

**Criteria:**
- Starts only after `docs/evidence/s4-review.txt` exists on disk.
- `~/.lope/memory.db` opens with `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;`, preserves existing rows, indexes findings in `findings_fts`, and supports all 5 `lope memory` subcommands (`FR25`, `NFR10`, `A11`).
- `--brain-context <query>` queries `makakoo search`, redacts, trims to `--brain-budget` (`1,200` tokens), and prepends once; `--brain-log` appends a Logseq bullet to today's Makakoo journal and runs `makakoo sync` (`FR26`, `FR27`, `A12`, `SC8`).
- `docs/evidence/s5-memory-brain.txt` is written on disk.

**Files:**
- `crates/lope-memory/src/store.rs`
- `crates/lope-memory/src/makakoo.rs`
- `docs/evidence/s5-memory-brain.txt`

**Tests:**
- `A11: cargo test -p lope-memory --test sqlite_wal_store`
- `A12 & SC8: cargo test -p lope-memory --test makakoo_brain_bridge`

---

### Phase 7: s6-council-deliberation

**Goal:** Implement the 7-stage adversarial Council deliberation engine (`lope deliberate` across all 6 templates and 3 depths) and the live Ratatui split-pane TUI viewer (`lope attach` / `--watch`) ([`S6`](docs/sprints/S6-council-deliberation.md)).

**Criteria:**
- Starts only after `docs/evidence/s5-memory-brain.txt` exists on disk.
- `lope deliberate` executes Intake → Positions → Anonymized `Response A/B/C` Critique → Revision → Primary Synthesis → Rubric Review → Minority Report across `adr`, `prd`, `rfc`, `build-vs-buy`, `migration-plan`, and `incident-review`, enforcing contributor quorum (`FR28`, `A13`, `SC9`).
- `lope attach [<run-id>]` renders live validator PTY buffers, status badges (`IDLE`, `WORKING`, `BLOCKED`, `DONE`), and keyboard/mouse focus in Ratatui (`FR34`, `SC10`).
- `docs/evidence/s6-deliberate-tui.txt` is written on disk.

**Files:**
- `crates/lope-engine/src/deliberate.rs`
- `crates/lope-tui/src/attach_ui.rs`
- `docs/evidence/s6-deliberate-tui.txt`

**Tests:**
- `A13 & SC9: cargo test -p lope-engine --test council_deliberation`
- `SC10: cargo test -p lope-tui --test attach_split_pane_render`

---

### Phase 8: s7-sprints-and-flow-graphs

**Goal:** Implement the multi-phase sprint engine (`negotiate --domain`, `execute`, `implement`, `audit`, `gate`, `check`) and the declarative Graphviz DOT workflow runner (`lope flow {run,validate,render,templates}`) ([`S7`](docs/sprints/S7-sprints-and-flow-graphs.md)).

**Criteria:**
- Starts only after `docs/evidence/s6-deliberate-tui.txt` exists on disk.
- `lope negotiate <goal> --domain {engineering,business,research}` drafts domain-tailored `SPRINT-<SLUG>.md` plans (`FR29`, `SC11`).
- `lope execute` and `lope implement` run phases with objective gates (`lope gate`/`check`), validator ensemble review, and `NEEDS_FIX` retry healing, producing scorecards via `lope audit` (`FR30`–`FR32`, `A14`, `SC12`).
- `lope flow` parses, validates, renders, and executes `.dot` graphs in parallel waves with barrier joins, blackboard interpolation, and strict `max_node_visits` cost bounds (`FR33`, `A15`, `SC13`).
- `docs/evidence/s7-sprints-flow.txt` is written on disk.

**Files:**
- `crates/lope-engine/src/sprint.rs`
- `crates/lope-engine/src/gates.rs`
- `crates/lope-flow/`
- `docs/evidence/s7-sprints-flow.txt`

**Tests:**
- `A14, SC11 & SC12: cargo test -p lope-engine --test sprint_lifecycle`
- `A15 & SC13: cargo test -p lope-flow --test dot_wave_runner`

---

### Phase 9: s8-glimmer-cutover-and-benchmarks

**Goal:** Build the stripped release binary (`target/release/lope`), preserve `/usr/local/bin/lope-py` fallback, cut over `/usr/local/bin/lope` to Super Lope, refresh host skills (`lope install`), verify full end-to-end compatibility with Glimmer (`makakoo-jungle`) and Makakoo OS, and record the Intel Mac benchmark comparison ([`S8`](docs/sprints/S8-glimmer-cutover-and-benchmarks.md)).

**Criteria:**
- Starts only after `docs/evidence/s7-sprints-flow.txt` exists on disk.
- `/usr/local/bin/lope-py` runs legacy Python Lope (`0.17.3`); `/usr/local/bin/lope` runs native Rust Super Lope (`2.0.0`) (`FR4`, `M8`).
- `env -i HOME="$HOME" PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin" makakoo version && lope version && lope --version` and `glimmer doctor` exit `0` (`SC1`, `A16`).
- Glimmer's companion and watched Council integration checks pass 100% (`NFR15`, `M5`, `M6`).
- Intel Mac benchmark table in `docs/evidence/s8-cutover-bench.txt` confirms `< 10ms` CLI startup, `< 25 MB` active fan-out RSS, `< 250ms` blocker detection, and `0` false `cleanup_failed` errors (`SC15`, `NFR2`).

**Files:**
- `/usr/local/bin/lope`
- `/usr/local/bin/lope-py`
- `docs/evidence/s8-cutover-bench.txt`

**Tests:**
- `A16 & SC1: glimmer doctor && env -i HOME="$HOME" PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin" lope version`
- `SC15: ./scripts/bench-intel-mac.sh`
