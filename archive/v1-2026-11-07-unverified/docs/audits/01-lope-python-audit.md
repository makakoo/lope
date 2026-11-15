# Audit 01: Python Lope (`0.17.3`)

[Audits Index](README.md) · Related: [02 — Rust Herdr Audit](02-herdr-rust-audit.md), [03 — Makakoo & Glimmer Seams](03-makakoo-and-glimmer-seams-audit.md), [Specs Index](../specs/README.md)

Verified against `/Users/sebastian/.lope` on `2026-11-07` (`HEAD` at `9cffa55`, version `0.17.3`).

---

## 1. Repository & Codebase Metrics

- **Location**: `/Users/sebastian/.lope`
- **Language**: Python 3 (`PYTHONPATH=/Users/sebastian/.lope python3 -m lope`)
- **Total Source Volume**: `61` Python files in `lope/` (`53` top-level modules + `8` modules in `lope/flow/`), totaling **`27,322` lines of code**.

### Complete Module Inventory (`lope/`)

| Module Group | Files (`LOC`) | Responsibility |
|---|---|---|
| **CLI & Dispatch** | `cli.py` (`5,461`), `__init__.py` (`88`), `__main__.py` (`3`), `logo.py` (`159`), `progress.py` (`170`), `selector.py` (`103`), `output.py` (`60`) | `argparse` definition for all 23 subcommands, interactive TUI selector, terminal progress spinners, and output formatting. |
| **Process & Supervision** | `supervisor.py` (`654`), `processes.py` (`349`), `jobs.py` (`931`), `runlock.py` (`152`), `runtime.py` (`676`), `invocation.py` (`298`), `retry_policy.py` (`160`), `latency.py` (`277`), `request_plan.py` (`665`) | Subprocess isolation, `python3 -m lope.supervisor`, process-group kill, run/call manifest registry (`~/.lope/state/sprints/`), budget leases, circuit-breaker retries, and input admission plans. |
| **Validators & Adapters** | `validators.py` (`2,097`), `generic_validators.py` (`585`), `http_worker.py` (`74`), `curl_parser.py` (`434`), `cli_discovery.py` (`180`), `config.py` (`463`), `provider_errors.py` (`35`), `verdict_events.py` (`190`), `verdict_repair.py` (`272`), `output_validity.py` (`64`) | Built-in CLI adapters (`claude`, `codex`, `gemini`, `opencode`, `aider`, plus 10 generic presets), generic HTTP/subprocess providers (`~/.lope/config.json`), `--from-curl` parser, and `---VERDICT---...---END---` parsing/repair. |
| **Review, Findings & Memory** | `review.py` (`530`), `findings.py` (`1,031`), `memory.py` (`902`), `sarif.py` (`192`), `exporters.py` (`39`), `redaction.py` (`77`), `diagnostics.py` (`54`) | Single-file and consensus review orchestration, finding extraction/deduplication/scoring, SQLite finding store (`~/.lope/memory.db`), SARIF 2.1.0 export, and secret redaction. |
| **Council Deliberation** | `deliberation.py` (`1,149`), `ensemble.py` (`315`), `synthesis.py` (`461`), `divide.py` (`789`) | 7-stage adversarial Council protocol (`adr`, `prd`, `rfc`, `build-vs-buy`, `migration-plan`, `incident-review`), anonymized peer critique, rubric scoring, minority report, and role-based division. |
| **Sprint & Self-Healing** | `negotiator.py` (`591`), `executor.py` (`602`), `implement.py` (`403`), `implementation_runner.py` (`229`), `healer.py` (`544`), `gates.py` (`487`), `auditor.py` (`182`), `journal.py` (`78`), `models.py` (`511`), `minimality.py` (`113`), `trust.py` (`142`) | Multi-round sprint drafting (`negotiate --domain`), validator-in-the-loop phase execution (`execute`), zero-human swarm implementation (`implement`), objective shell gates (`gate`, `check`), and sprint scorecards (`audit`). |
| **Declarative Flow Engine** | `flow/runner.py` (`638`), `flow/model.py` (`552`), `flow/dot.py` (`290`), `flow/validate.py` (`211`), `flow/report.py` (`186`), `flow/templates.py` (`147`), `flow/stylesheet.py` (`113`), `flow/render.py` (`93`), `flow/__init__.py` (`48`) | Graphviz DOT parser, blackboard variable interpolation, wave-based parallel fan-out / barrier fan-in execution, conditional edge routing, and visit-cap cost bounds. |
| **Makakoo & Host Bridges** | `makakoo_bridge.py` (`414`), `makakoo_adapter.py` (`220`), `caveman.py` (`53`), `update.py` (`321`), `resources.py` (`15`) | `--brain-context <query>`, `--brain-log` journal writer + `makakoo sync`, `LOPE_BRAIN_AUTOMEMORY`, switchAILocal adapter, and self-update/skill sync. |

---

## 2. Complete CLI Verb Inventory (`23` Subcommands)

Every subcommand below is exposed by `lope/cli.py` and must be preserved in Super Lope:

1. **`negotiate`**: Draft a `SPRINT-<SLUG>.md` plan via multi-round validator negotiation (`--domain {engineering,business,research}`, `--rounds`, `--out`, `--brain-context`, `--brain-log`).
2. **`execute`**: Execute a sprint document phase-by-phase with validator-in-the-loop retry (`--max-retries`, `--phase`).
3. **`implement`**: Zero-human swarm sprint execution with primary implementer + escalation roster in an isolated worktree.
4. **`audit`**: Generate a sprint scorecard (`PASS`/`NEEDS_FIX`/`FAIL`, confidence, duration) and optionally log to `~/.lope/journal.jsonl`.
5. **`ask`**: Fan out a freeform question to all configured validators and print per-model answers (`--timeout`, `--json`, `--markdown`, `--brain-context`).
6. **`review`**: Critique a single target file across validators (`--consensus [quorum|unanimous|any]`, `--format {text,json,markdown,markdown-pr,sarif}`, `--output <path>`, `--focus <lens>`, `--remember`, `--brain-context <query>`, `--brain-log`).
7. **`vote`**: Tally discrete option votes across validators (`--options "A,B,C"`, `--criteria`).
8. **`compare`**: A/B comparison of two files against mandatory `--criteria`.
9. **`pipe`**: Read prompt from `stdin` and fan out to validators (`--require-all`).
10. **`team`**: Manage validator roster (`list`, `add`, `remove`, `test`, `enable`, `disable`, `--from-curl`).
11. **`deliberate`**: Run the 7-stage adversarial Council deliberation (`--template {adr,prd,rfc,build-vs-buy,migration-plan,incident-review}`, `--depth {quick,standard,deep}`, `--output`).
12. **`memory`**: Query the persistent SQLite finding store (`stats`, `search`, `file`, `hotspots`, `forget`).
13. **`gate`**: Run objective verification gates (test commands, linters, file checks).
14. **`check`**: Single-command readiness and syntax verification check.
15. **`status`**: Print active primary, enabled validators, timeout/budget settings, and run manifest counts (`active`, `abandoned`, `completed`).
16. **`jobs`**: Inspect and maintain run manifests (`list`, `show`, `prune --stale`, `reconcile`).
17. **`configure`**: Interactive or flag-driven configuration of primary, validators, parallel mode, and timeouts.
18. **`install`**: Install host skills/slash-commands into Claude Code, Cursor, Codex, Gemini, OpenCode, and Antigravity.
19. **`update`** / 20. **`upgrade`**: Pull clean upstream release and refresh host skills.
20. **`version`**: Print Lope version.
21. **`docs`**: Print complete built-in CLI reference documentation.
22. **`flow`**: Parse, validate, render, or execute declarative DOT workflow graphs (`run`, `validate`, `render`, `templates`).

---

## 3. Forensic Analysis of Python Lope's Performance & Timeout Failures

### 3.1 The Double-Interpreter + Busy-Poll Penalty (`lope/processes.py`)
In `lope/processes.py:231-270`, every single call to `run_subprocess_group()`:
1. Creates a temporary directory (`lope-call-*` or registry call dir) with mode `0700`.
2. Hashes the command and updates the JSON manifest on disk (`registry.update_call`).
3. Spawns `subprocess.Popen([sys.executable, "-m", "lope.supervisor", str(result_path)])`.
4. Writes the JSON spec over non-blocking `stdin` using `select.select`.
5. Enters a busy-wait loop:
   ```python
   while wire_complete and supervisor.poll() is None and time.monotonic() < call_deadline:
       time.sleep(0.02)
   ```
   When `5` validators run in parallel (`ThreadPoolExecutor(max_workers=5)`), 5 Python threads wake up 50 times per second each (`250 wakeups/sec`), while 5 separate `python3 -m lope.supervisor` interpreters run `selectors.DefaultSelector()` loops and spawn 5 heavy Node.js/Python CLIs (`codex`, `claude`, `grok`, `agy`, `kimi`).

### 3.2 The macOS Darwin Reaping Bug (`lope/supervisor.py:102-118`, `476-495`)
In `lope/supervisor.py`:
- When the provider process finishes (`if proc.poll() is not None:` at line `476`), the supervisor unconditionally calls `_terminate_and_reap(proc)` (line `481`).
- `_terminate_and_reap` sends `SIGKILL` to the process group and calls `_wait_group_exit(proc.pid, deadline)` with a `0.7s` total budget (`CLEANUP_TIMEOUT_SECONDS = 0.7`).
- `_wait_group_exit` calls `_process_group_count(pgid)` (lines `102-118`):
  ```python
  try:
      os.killpg(pgid, 0)
  except ProcessLookupError:
      return 0
  ...
  if sys.platform == "darwin":
      return 1
  ```
- Because macOS (`darwin`) skips `ps` inspection and assumes the process group has live members whenever `os.killpg(pgid, 0)` does not raise `ProcessLookupError`, any zombie child or slow-closing MCP helper in that process group causes `_terminate_and_reap` to return `"cleanup_failed:descendants_live"`.
- Lines `489-491` then overwrite `outcome = "ok"` with:
  ```python
  elif not (cleanup == "clean" or cleanup.startswith("clean:")):
      outcome = "cleanup_failed"
      reason = "provider exited but owned descendants could not be reaped"
  ```
- Finally, `lope/processes.py:314` raises `ProcessCompletionError` and **discards `stdout`**, even when `returncode == 0` and `stdout` contains the complete validator critique or `---VERDICT---` block.

### 3.3 Blind Pipe Hang on Interactive Prompts (`lope/supervisor.py:355-368`)
- `_spawn_provider` sets `stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE`.
- Modern agent CLIs (`claude`, `codex`, `agy`, `grok`, `opencode`, `kimi`) frequently emit TUI prompts when launched in a new directory or after a version bump:
  - Folder trust confirmation (`"Do you trust the contents of this directory?"`)
  - Auto-update prompt (`"Update available! Press enter to continue"`)
  - Tool / MCP server permission dialog
- Without a PTY or screen-buffer parser, Python Lope cannot detect that the CLI is waiting for key input (`Blocked`). The call hangs until the full `timeout` (`120s`–`900s`) expires.

---

[Audits Index](README.md) · Next: [02 — Rust Herdr Audit](02-herdr-rust-audit.md)
