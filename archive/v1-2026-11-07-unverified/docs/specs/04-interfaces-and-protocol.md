# Spec 04: Interfaces, Protocol & Schemas

[Specs Index](README.md) · Previous: [03 — Requirements & Acceptance](03-requirements-and-acceptance.md) · Next: [Sprints Index](../sprints/README.md)

---

## 1. CLI Surface & Exit Codes (`/usr/local/bin/lope`)

### 1.1 Global Flags & Subcommands
```text
lope [GLOBAL_OPTIONS] <SUBCOMMAND> [ARGS...]

Global Options:
  -V, --version                  Print version information and exit 0
  -h, --help                     Print help information
  -c, --config <PATH>            Override config file (default: ~/.lope/config.json)
      --machine <HOST>           Target a saved remote SSH machine endpoint
      --no-daemon                Force ephemeral Tier 1/2 execution without connecting to loped
      --watch                    Attach live Ratatui split-pane TUI during execution
      --respect-provider-timeout Allow provider-specific timeout to exceed call ceiling

Subcommands (23 Legacy + 4 New):
  negotiate    Draft a sprint plan (SPRINT-<SLUG>.md) via multi-round validator negotiation
  execute      Execute a sprint plan phase-by-phase with validator-in-the-loop retry
  implement    Autonomous zero-human sprint execution with primary + escalation roster
  audit        Generate a sprint scorecard from execution artifacts
  ask          Ask every configured validator the same question
  review       Critique a target file across validators (single-shot or --consensus)
  vote         Tally discrete option picks across validators
  compare      Compare two files against mandatory --criteria
  pipe         Read prompt from stdin and fan out to validators
  team         Manage validator roster (list, add, remove, test, enable, disable, --from-curl)
  deliberate   Run 7-stage adversarial Council deliberation (adr, prd, rfc, build-vs-buy, migration-plan, incident-review)
  memory       Query persistent SQLite finding store (stats, search, file, hotspots, forget)
  gate         Execute objective shell/file verification gates
  check        Run readiness and gate checks
  status       Display active validator team, settings, warm pool, and manifest summary
  jobs         Inspect and prune run manifests (list, show, prune --stale, reconcile)
  configure    Update primary, validator roster, parallel mode, and timeouts
  install      Install/update Lope skills across host CLIs
  update       Update Super Lope release binary and skills
  upgrade      Alias for update
  version      Print version string (e.g. "lope 2.0.0 (super-lope; compat 0.17.3)")
  docs         Print complete CLI and architecture reference
  flow         Run, validate, or render declarative Graphviz DOT workflows
  daemon       [NEW] Manage background loped server (start, stop, restart, status)
  pool         [NEW] Manage warm agent PTY pool (list, warm, drain, reset)
  attach       [NEW] Attach live Ratatui split-pane TUI to an active or recent run
  detect       [NEW] Run 22-agent screen-state detector on a pane or capture file
```

### 1.2 Standard Exit Codes
| Exit Code | Meaning |
|---|---|
| `0` | Success (`PASS` verdict, completed review/deliberation/query, clean `doctor`/`status`). |
| `1` | Execution failure (`FAIL` / `NEEDS_FIX` after retries exhausted, quorum not met, or runtime error). |
| `2` | CLI usage error, missing file, or admission policy rejection (`PlanAction::Reject` / `InputLimitExceeded`). |
| `3` | Aborted due to `OutcomeClass::Blocked` when `--fail-on-blocked` is set. |
| `130` | Interrupted by `SIGINT` (`Ctrl+C`) with clean child process-group teardown. |

---

## 2. Exact `argv` Contracts for Glimmer & Makakoo OS

### 2.1 `lope review` Contract
```bash
lope review <file> \
  [--consensus [quorum|unanimous|any]] \
  [--format {text|json|markdown|markdown-pr|sarif}] \
  [--output <path>] \
  [--focus <lens-or-text>] \
  [--brain-context <query>] \
  [--brain-budget <tokens>] \
  [--brain-log] \
  [--remember] \
  [--include-raw] \
  [--timeout <seconds>]
```
- **Compatibility rule**: `--consensus` must work both as a boolean flag (when followed by another `--flag` or `<file>`, defaulting to `quorum`) and as an option taking `quorum|unanimous|any` (`num_args = 0..=1, default_missing_value = "quorum"` in `clap`).
- **Role Lens Map (`--focus`)**:
  - `product` / `Product & UX` → Product scope, user value, UX consistency, edge cases.
  - `sales` / `Sales & Commercial` → Commercial positioning, pricing clarity, customer objections, ROI.
  - `tax` / `Tax & Fiscal` → Tax compliance, invoice/VERI*FACTU rules, deductibility, statutory deadlines.
  - `finance` / `Finance & Investment` → Unit economics, cashflow impact, risk exposure, assumptions.
  - `legal` / `Legal, Regulatory & Assurance` → Contractual liability, IP/licensing, GDPR/privacy, regulatory audit.
  - `research` / `Research & Evidence` → Methodology rigor, statistical validity, citation verification, confounds.
  - `engineering` / `Engineering / Code` → Correctness, concurrency, security, performance, test coverage.
  - Any other string → Passed verbatim as the custom review focus prompt.

### 2.2 `lope negotiate` & `lope implement` Contract
```bash
lope negotiate "<goal>" \
  [--domain {engineering|business|research}] \
  [--rounds <1..5>] \
  [--out <path>] \
  [--brain-context <query>] \
  [--brain-log]

lope implement <sprint-doc-path> \
  [--primary <validator>] \
  [--escalate <validator-list>] \
  [--max-retries <n>]
```

---

## 3. Configuration Schema (`~/.lope/config.json`)

Super Lope reads and writes `~/.lope/config.json` with backward-compatible fields plus optional `pool` and `blocker_policy` blocks:

```json
{
  "primary": "codex",
  "validators": ["codex", "claude", "grok", "agy", "kimi", "claude-safe-review"],
  "parallel": true,
  "max_parallel_ptys": 4,
  "timeout": 900,
  "request_policy": "auto",
  "max_chunks": 32,
  "max_calls": 96,
  "max_input_bytes": 16777216,
  "pool": {
    "enabled": true,
    "warm_agents": ["claude", "codex"],
    "idle_ttl_seconds": 1800
  },
  "blocker_policy": {
    "auto_dismiss_updates": true,
    "auto_trust_scratch_dirs": true,
    "fail_fast_on_read_only_permission": true,
    "detection_debounce_ms": 50
  },
  "providers": [
    {
      "name": "switchai-fast",
      "type": "http",
      "url": "http://127.0.0.1:18080/v1/chat/completions",
      "headers": {
        "Authorization": "Bearer ${AIL_API_KEY}",
        "Content-Type": "application/json"
      },
      "body": {
        "model": "gemini-3.6-flash",
        "messages": [{"role": "user", "content": "{prompt}"}]
      },
      "response_path": "choices.0.message.content",
      "timeout": 120
    },
    {
      "name": "grok",
      "type": "subprocess",
      "command": ["grok", "-p", "{prompt}"],
      "execution_tier": "pty_oneshot",
      "detect_manifest": "grok",
      "timeout": 180
    }
  ]
}
```

---

## 4. Daemon Socket Protocol (`lope.socket.v1` at `~/.lope/run/lope.sock`)

Line-delimited JSON-RPC 2.0 over Unix domain socket (`0600`, peer UID verified via `LOCAL_PEERCRED`):

### Request Envelope
```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "method": "pool.lease_and_prompt",
  "params": {
    "validator": "claude",
    "prompt": "Review this function...",
    "cwd": "/Users/sebastian/Projects/apps/super-lope",
    "read_only": true,
    "timeout_ms": 120000
  }
}
```

### Response Envelope
```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "result": {
    "outcome": "ok",
    "validator": "claude",
    "tier": "warm_pool",
    "pane_id": "w1:p2",
    "elapsed_ms": 4120,
    "blockers_remediated": [],
    "text": "- [MEDIUM] src/lib.rs:42 — Missing bounds check (confidence: 0.88)"
  }
}
```

### Supported RPC Methods
- `daemon.ping` / `daemon.status` / `daemon.shutdown`
- `pool.list` / `pool.warm` / `pool.drain` / `pool.reset` / `pool.lease_and_prompt`
- `run.list` / `run.inspect` / `run.cancel` / `run.subscribe_events`
- `detect.evaluate_pane` / `detect.evaluate_snapshot`

---

## 5. Verdict Block & SQLite Memory Schemas

### 5.1 Canonical `---VERDICT---` Contract
```text
---VERDICT---
status: PASS | NEEDS_FIX | FAIL
confidence: 0.0-1.0
rationale: 1-3 sentences, terse; cite file:line where relevant
required_fixes:
  - fix 1 (only when NEEDS_FIX or FAIL)
---END---
```

### 5.2 SQLite Schema (`~/.lope/memory.db`)
Preserves 100% table compatibility with `lope/memory.py` while adding an FTS5 index:
```sql
CREATE TABLE IF NOT EXISTS findings (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    target_file TEXT NOT NULL,
    line_start INTEGER,
    line_end INTEGER,
    severity TEXT NOT NULL,
    category TEXT,
    message TEXT NOT NULL,
    confidence REAL NOT NULL,
    validators_json TEXT NOT NULL,
    consensus_score REAL NOT NULL,
    fingerprint TEXT NOT NULL,
    run_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_findings_target ON findings(target_file);
CREATE INDEX IF NOT EXISTS idx_findings_fingerprint ON findings(fingerprint);

CREATE VIRTUAL TABLE IF NOT EXISTS findings_fts USING fts5(
    target_file,
    severity,
    category,
    message,
    content='findings',
    content_rowid='rowid'
);
```

---

[Specs Index](README.md) · Previous: [03 — Requirements & Acceptance](03-requirements-and-acceptance.md) · Next: [Sprints Index](../sprints/README.md)
