# Audit 01 — Python Lope

[Audits](README.md) · Next: [02 — Herdr](02-herdr-rust-audit.md)

Verified against `/Users/sebastian/.lope` on 2026-11-07. HEAD `9cffa55` (`__version__ = "0.17.2"`, `lope/__init__.py`), with an **uncommitted** working tree that bumps to `0.17.3` and changes 15 files (`git diff --stat`: +109/−20; includes `cli.py`, `supervisor.py`, `processes.py`, `output.py`, `generic_validators.py`, `invocation.py`, `retry_policy.py`). Licence MIT (`LICENSE`, `pyproject.toml:6`).

---

## 1. Codebase

62 Python files, 27,322 lines: 53 in `lope/`, 9 in `lope/flow/` (including `__init__.py`). Largest modules: `cli.py` 5,461 · `validators.py` 2,097 · `deliberation.py` 1,149 · `findings.py` 1,031 · `jobs.py` 931 · `memory.py` 902 · `divide.py` 789 · `runtime.py` 676 · `request_plan.py` 665 · `supervisor.py` 654.

| Group | Modules |
|---|---|
| CLI & output | `cli.py`, `__main__.py`, `__init__.py`, `logo.py`, `progress.py` (writes to **stderr**, `progress.py:29`), `selector.py`, `output.py` |
| Process supervision & budgets | `supervisor.py`, `processes.py`, `jobs.py`, `runlock.py`, `runtime.py`, `invocation.py`, `retry_policy.py`, `latency.py`, `request_plan.py` |
| Validators | `validators.py`, `generic_validators.py`, `http_worker.py`, `curl_parser.py`, `cli_discovery.py`, `config.py`, `provider_errors.py`, `verdict_events.py`, `verdict_repair.py`, `output_validity.py`, `makakoo_adapter.py` (validators backed by `makakoo adapter call`; **not** a switchAILocal adapter) |
| Review & memory | `review.py`, `findings.py`, `memory.py`, `sarif.py`, `exporters.py`, `redaction.py`, `diagnostics.py`, `minimality.py`, `divide.py` (`--divide`/`--roles`) |
| Council | `deliberation.py`, `ensemble.py`, `synthesis.py` |
| Sprint & self-healing | `negotiator.py`, `executor.py`, `implement.py`, `implementation_runner.py`, `healer.py`, `gates.py`, `trust.py`, `auditor.py`, `journal.py`, `models.py` |
| Flow | `flow/{__init__,dot,model,validate,runner,report,templates,stylesheet,render}.py` |
| Host bridges | `makakoo_bridge.py`, `caveman.py`, `update.py`, `resources.py` |

There is **no Headroom code** in `lope/`; Headroom appears only in the installer (`install --with-headroom`) and the `lope-headroom` skill. There is no MCP server.

The test suite (`tests/`, 56 entries including `conftest.py` and `fixtures/`) covers curl parsing, config layering, consensus review, SARIF, memory, flow, deliberation, jobs, run lock, retry, request plans, verdict repair and more. It is the best available oracle for parity fixtures.

---

## 2. CLI surface

The authoritative capture is [`../reference/python-lope-0.17.3-cli-help.txt`](../reference/python-lope-0.17.3-cli-help.txt) (every verb and nested subcommand `--help`). Summary:

- **Top level:** only `-h`. No `--version`, `-V`, `-c/--config` or any global option (`cli.py:194-211`).
- **23 verbs:** `negotiate execute implement audit ask review vote compare pipe team deliberate memory gate check status jobs configure install update upgrade version docs flow`.
- **Nested subcommands:**
  - `team {list, enable, disable, add, remove, test, health}`
  - `memory {stats, search, file, hotspots, forget, gates}`
  - `gate {save, check}`
  - `jobs {list, reap, prune, kill}`
  - `flow {run, validate (alias lint), render, init, list}`
- **Shared flag groups** (`cli.py:218-251`):
  - Pool flags `--validators --primary --timeout --respect-provider-timeout --run-timeout | --allow-unbounded-run --max-calls --max-chunks --request-policy {auto,direct,chunk} --parallel | --sequential`, on negotiate, execute, implement, audit, ask, review, vote, compare, pipe, deliberate and `flow run`.
  - `--synth --anonymous` on ask, review, vote, compare and pipe.
  - `--brain-context QUERY --brain-budget N --brain-log` on negotiate, ask, review, pipe and deliberate.
- **Shapes that matter for compatibility:**
  - `review FILE`: exactly one positional, which may be a directory with `--divide files`.
    - `--consensus` (alias `--structured`) is **`store_true`** (`cli.py:369`).
    - Also: `--format {text,json,markdown,markdown-pr,sarif}`, `--output/-o` (`cli.py:380`), `--focus` (free text), `--json`, `--min-consensus`, `--similarity`, `--include-raw`, `--remember`, `--divide {files,hunks}`, `--roles`.
  - `negotiate GOAL`: `--max-rounds` (default 3, `cli.py:278`; no `--rounds`), `--context`, `--context-file`, `--domain {engineering,business,research}`, `--out`.
  - `implement SPRINT_DOC`: `--agents --escalate-to --phase --gates --gate-config --trust --dry-run --interactive`. There is no `--primary-only`, `--escalate` or `--max-retries`. With no roster flags in a TTY it prompts (`implement.py:162-185`).
  - `execute SPRINT_DOC`: `--phase --manual --gates --gate-config --trust`. Retries are fixed at 3 per phase (`cli.py:2839`).
  - `deliberate TEMPLATE SCENARIO`: the template is **positional**; the scenario is a path or `-`. Flags: `--depth`, `--out DIR` (default `lope-runs/<ts>-<template>/`), `--no-anonymize`, `--minority-report`, `--human-questions {never,blocking,always}`, `--json`.
  - `vote PROMPT --options …` (required); `compare A B [--criteria …]` (**optional**, default "correctness and clarity", `cli.py:421`); `ask QUESTION [--context]`; `pipe [--require-all]`.
  - `check` and `gate save|check` have their own `--config` (the rules file). A global `-c/--config` would collide with it.
  - `install [--host]`; `update/upgrade [--dry-run --method {auto,git,pip} --host --skip-install --allow-dirty]`. Hosts: `claude codex gemini opencode cursor vibe qwen pi all` (`install:5`).
- **Exit codes** (123 exit sites in `cli.py`):

  | Code | Meaning |
  |---|---|
  | 0 | Success |
  | 1 | Failure |
  | 2 | Usage error or nested-run refusal |
  | 3 | Untrusted gate commands (`cli.py:1147,1177,1247`) or `--manual` without a TTY (`:2728`) |
  | 75 | Run lock held (`runlock.py:122,134`) |
  | 124 | Run deadline exhausted (`cli.py:1900`) or installer timeout (`:1851`) |
  | passthrough | `install` returns the installer's own exit code (`:1855`) |
  | 130 / 143 | SIGINT / SIGTERM; there is no handler, so the process dies by signal |

- **Nesting guard:** any `LOPE_IMPLEMENTATION_DEPTH > 0` (or a non-integer value) makes **every** command print `error: nested Lope orchestration is disabled inside an implementation writer` and exit 2 (`cli.py:182-192`). Writers get the incremented depth (`processes.py:184-190`).
- **`lope version`:** a blank line, a 4-line box-drawing "L O P E" logo, then `    sprint runner v0.17.3` (`cli.py:819-825`, `logo.py:80-90`). Takes 1.00–1.25 s wall on the dogfood Mac.
- **`lope status`:** prints the team and config, then a `Jobs:` section (`cli.py:1690-1717`). It shows either `  0 active or stale Lope-owned runs`, or `  {N} manifest(s): <class>=<n>, …` (sorted) followed by one line per run. Classes are `active`, `unresponsive`, `abandoned`, `cleanup_failed` and `ownership_unverified`.
- **`review` stdout:** before the report it prints a header block (`Lope consensus review: <file>`, `Validators:`, `Focus:`, `Format:`, `Timeout:`) to stdout (`cli.py:4061-4065`). Progress goes to stderr.

---

## 3. State and files under `$LOPE_HOME` (`~/.lope`)

| Path | Owner / purpose | Notes |
|---|---|---|
| `config.json` (+ `config.json.lock`, `*.bak*`) | team config | see §4 |
| `memory.db` (+ `-wal`, `-shm`) | finding store | see §5 |
| `runs/{active,completed,work}/`, `runs/registry.lock` | run/call manifest registry (`jobs.py:225-228`) | **There is no `state/sprints/` directory.** Live: 0 active, 137 completed. |
| `run.lock` | global run lock (`runlock.py`) | `LOPE_RUN_LOCK`, `LOPE_RUN_LOCK_PATH`, `LOPE_RUN_LOCK_WAIT` |
| `lope-runs/` | deliberate/flow output default | `LOPE_RUN_OUTPUT_DIR` |
| `journal/YYYY_MM_DD.md` | `lope audit` scorecards, on by default; `--no-journal` disables (`auditor.py:33-40`) | |
| `journal.jsonl` | heal-event log (`journal.py:27-30`) | absent on this machine |
| `latency.json` | latency ledger, adaptive timeout ceilings (`latency.py`) | `LOPE_LATENCY` |
| `trusted_gates.json` | gate trust store (`trust.py`) | `LOPE_TRUST_GATES` |
| `skills/`, `commands/`, `hooks/`, `.claude-plugin/`, `.cursor-plugin/`, `gemini-extension.json` | host integration assets installed by `./install` | |

All runtime state paths above are in `~/.lope/.gitignore`. `~/.lope` is a git checkout of the Python repo, so **any new untracked path makes the tree dirty**. That would make `glimmer install` refuse to run and Glimmer show a dirty Lope.

**Per-project state:** `./.lope/config.json` (config layering), `./.lope/rules.json` (gate baselines), and `.lope/reports/` (written by Glimmer, not Lope).

**Manifest privacy:** `_FORBIDDEN_MANIFEST_KEYS` (`jobs.py:37-48`) = `prompt, prompt_text, response, response_body, raw_response, secret, credentials, argv, environment, env`.

---

## 4. Configuration rules (`config.py`)

- **`LOPE_HOME` resolution is inconsistent (found in S0).** `jobs.py:56` and `runlock.py:36` use `os.environ.get("LOPE_HOME") or ~/.lope`, so an empty value means the default. `config.py:66`, `journal.py:29`, `latency.py:75`, `trust.py:41` and `auditor.py:38` use `os.environ.get("LOPE_HOME", default)`, so an empty value becomes `""` and paths become cwd-relative. S1 must decide whether Rust reproduces this per file (parity) or treats empty as unset everywhere (DEC needed).

- `_parse_dict` returns `None` for any file without `"version": 1` (`config.py:76-82`). Such a config is silently ignored.
- **Recognised keys:** `version, validators (list), primary, timeout, parallel, providers, learned_adapters, run_timeout, allow_unbounded_run, max_calls, max_input_bytes, max_output_bytes, max_chunks, request_policy`.
- **`save()` writes only the recognised keys** (`config.py:394-433`). Optional keys are omitted when they hold default values. The write takes an exclusive `flock` on `config.json.lock`, writes a temp file, fsyncs, then renames. **Unknown keys are dropped on every save.**
- **Layering:** `~/.lope/config.json`, then `./.lope/config.json`, then env overrides (`LOPE_VALIDATORS`, `LOPE_PRIMARY`, `LOPE_TIMEOUT`, `LOPE_PARALLEL`, `LOPE_SEQUENTIAL`, `LOPE_REQUEST_POLICY`, `LOPE_ALLOW_UNBOUNDED_RUN`, `LOPE_MAX_*`, `LOPE_RUN_TIMEOUT`).
- **Provider fields:**
  - subprocess: `name, type, command, stdin, prompt_wrapper, timeout, review_command, review_workdir, review_stdin` (`generic_validators.py:120-170, 232-244`)
  - http: `url, headers, body, response_path, prompt_wrapper, timeout, max_tokens, response_limit` (`:452-464`)
- **Live config (key names only):** top level `parallel, primary, providers, timeout, validators, version`. Validators `codex, claude, grok, agy, kimi, claude-safe-review`; primary `codex`. 6 providers, all subprocess.
- **Other writers:** Makakoo `onboarding.py` appends to `validators` (Audit 03 §2).

---

## 5. `memory.db`

The full DDL is in [`../reference/python-lope-memory-db-schema.sql`](../reference/python-lope-memory-db-schema.sql).

- **Tables:** `findings`, `review_sessions`, `session_findings`, `gate_sessions`, `verdict_events`. There is no `runs` table and no FTS table.
- **`findings` columns:** `id INTEGER PK AUTOINCREMENT, hash UNIQUE, message, file, line, end_line, severity, category, confidence, consensus_score, consensus_level, agreement_count, total_validators, detected_by_json, evidence_json, first_seen_at, last_seen_at, seen_count`.
- **Version:** `PRAGMA user_version = 1`. Python refuses a database with a higher version (`memory.py:350-357`).
- **Connection:** WAL and `synchronous=NORMAL` (`memory.py:461-462`), plus the default 5-second connect timeout.
- **Search:** `LIKE` (`memory.py:662-668`).
- **Env:** `LOPE_MEMORY=off` disables the store; `LOPE_MEMORY_DB` relocates it.
- **Location (found in S0):** `$LOPE_MEMORY_DB`, else `$HOME/.lope/memory.db`. It does **not** follow `$LOPE_HOME` (`memory.py:44-46, 75-82`).

---

## 6. Validators and invocation

- **Built-in classes** (`validators.py:1987-1993`): `claude, opencode, gemini, codex, aider`.
- **Presets** (`cli_discovery.py`): `vibe, ollama, goose, interpreter, llama-cpp, gh-copilot, amazon-q, pi, qwen, agy`.
- **Further lookup tiers:** learned adapters (healer) and Makakoo adapters. `grok`, `kimi` and `cursor` are **not** built-ins; on this machine grok and kimi are user-defined subprocess providers.
- **Invocation:**
  - `claude --print --safe-mode --no-session-persistence --output-format text [$LOPE_CLAUDE_ARGS]`, prompt on **stdin** (`validators.py:215-232, 1629`)
  - `codex exec --ignore-user-config --skip-git-repo-check -s read-only|workspace-write -m $LOPE_CODEX_MODEL -c model_reasoning_effort="low"`, prompt on **stdin**, working directory from `LOPE_WORKDIR` (`:1703, 1756-1772`)
  - `gemini --prompt <p> --output-format json` (`:1182`)
  - `opencode run [$LOPE_OPENCODE_ARGS] --format json <p>`, JSON event stream (`:472`)
  - `aider --message-file <0600 tmp> --no-git --no-auto-commits --yes` (`:1887`)
  - Presets substitute `{prompt}` into argv, e.g. `pi --no-session --offline --no-tools -p {prompt}`.
- **Makakoo adapter validators:** `makakoo adapter call <name> --timeout N`, prompt on stdin, registry `~/.makakoo/adapters/registered/*.toml` (`MAKAKOO_ADAPTERS_HOME`), disabled with `LOPE_MAKAKOO_ADAPTERS=0` (`makakoo_adapter.py:1-17, 44-59, 95-106`).
- **Verdict contract:** a `---VERDICT---…---END---` block, parsed JSON-first with a YAML-ish fallback (`validators.py:4-16, 247`). One-shot structural repair lives in `verdict_repair.py`.

---

## 7. Process runtime (`processes.py`, `supervisor.py`)

- **Per-call supervisor:** each call spawns `python3 -m lope.supervisor <result_path>` in a new session (`processes.py:231-245`). The parent writes a JSON spec to it, then waits in `time.sleep(0.02)` until it exits (`:266-267`).
- **Provider spawn:** argv only (`shell=False`), stdin/stdout/stderr are pipes, `start_new_session=True` (`supervisor.py:351-367`). Stdin is closed once the input is written (`:454, :533`).
- **Limits** (`processes.py:26-32`): stdout 2 MiB, stderr 512 KiB, argv payload 128 KiB.
- **On leader exit** (`supervisor.py:476-495`):
  - `_terminate_and_reap` signals the group (SIGTERM, then SIGKILL), waits for the group to empty within `CLEANUP_TIMEOUT_SECONDS = 0.7` (`:36`), then drains pipes for at most 0.5 s.
  - If cleanup isn't `clean*`, the outcome becomes `cleanup_failed`. If pipes are still open, it becomes `output_drain_timeout`.
  - `processes.py` raises `ProcessCompletionError` for either outcome.
- **Uncommitted WIP:**
  - Adds the Darwin `return 1` shortcut in `_process_group_count` (`supervisor.py:117`) and the `group_absent` short-circuit.
  - Records `stdout_complete` and `stdout_truncated`.
  - Turns completion failures into `CompletionDiagnostic`, which reports status `UNVERIFIED` and metadata in JSON output (`output.py`). The body is shown only with `LOPE_RECOVER_UNVERIFIED_OUTPUT=1`.
  - This is a deliberate **fail-closed** policy.
- **Live outcomes** (139 call records in `~/.lope/runs`): `ok` 100, `provider_timeout` 27 (claude 18, agy 5, grok 4), `launch_error` 3, `tool_permission_denied` 1, `cancelled` 1. Cleanup results: `clean` 237, `clean:process_group` 28, `cleanup_failed:inspection_unknown` 4.

---

- **Closeout bug with symlinked ancestors (found in S0, committed code).** `jobs.py:507-522` (`is_owned_path`) resolves the run root but walks the unresolved candidate path upward until it equals that root. If any ancestor of `$LOPE_HOME` is a symlink, the walk passes the root, meets the symlink and rejects the path. `close_run` (`jobs.py:836-872`) then reports "owned scratch cleanup failure", and `cli.py:1930-1938` exits 1 after successful calls. It does not affect `/Users/sebastian/.lope`, which has no symlinked ancestor. Patch set PL-5.

## 8. Review, findings, deliberation, sprint, flow — facts the spec depends on

- **`--focus`** is free text. The only built-in aliases are the minimality ones (`over-engineering`, `overengineering`, `minimality`, `lazy-build`, `lazy build`, `minimality.py:98-113`). Role lenses exist only behind `--roles`: `security correctness performance tests api-design docs ops ux` plus aliases (`divide.py:588-710`). There are no product, sales, tax, finance or legal lenses.
- **Consensus buckets** (`findings.py:119-127, 923-940`):
  - `confirmed`: agreement ≥ 0.60 and confidence ≥ 0.65
  - `likely`: ≥ 0.40 and ≥ 0.55
  - `needs-verification`: ≥ 0.20, or severity critical/high
  - `unverified`
  - `disputed`: reserved
- `--min-consensus` filters on `consensus_score` (`review.py:253-254`).
- **Deliberation** (`deliberation.py`):
  - 7 stages; `quick` skips critique and revision.
  - Quorum is `expected // 2 + 1` (`:71`).
  - Anonymised labels `Response A…`, then `AA` (`:468-473`).
  - Limits: scenario 48 KB, turn 32 KB, peer block 64 KB, 5 workers (`:56-59`).
  - `human_questions` is defined but not consumed.
- **Negotiate:**
  - Domain sections (`models.py:139-155`): engineering = Files/Tests, business = Deliverables/Success Metrics, research = Artifacts/Validation Criteria.
  - Output is `SPRINT-<SLUG>.md`; the slug takes the goal cut to 60 characters (`negotiator.py:414-418`, `cli.py:2633`).
  - Writes a `.rounds` sidecar (`cli.py:2466`).
- **Flow** (`flow/model.py`):
  - Node types `start, agent, review, judge, script, gate, exit` (`:45-51`), plus aliases (implement, ensemble, consensus, steer, verify, human, approve) and `shape=` mappings.
  - Caps: `max_visits` default 3 (`:142`), `max_node_visits` default `max(50, 8 × nodes)` (`:239-248`), `max_model_calls` (`:251`).
  - Interpolation: `$task` / `$VAR` and single-brace `{node.key}`, where key is one of `outcome detail out out_ref verdict rationale` (`:377-380, 501-512`).
  - Templates: `consensus`, `judge-loop`, `review-gate` (`templates.py:130-134`).

---

## 9. Makakoo bridge (`makakoo_bridge.py`)

- **`--brain-context Q`:** runs `makakoo search Q` through the supervised subprocess with a 30 s timeout (`:172-222`). There is no SQLite fallback. Output is dropped only when it is empty (`:209-211`). Budget is `1200` tokens × 4 = 4,800 characters, minimum 400 (`:43-44, 213`).
- **`--brain-log`:** appends to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md`.
  - The date is **UTC** (`:250-252`).
  - Every line is forced to start with `- ` (`:279-287`).
  - Bullets carry `[[Lope]] … [[Makakoo OS]]`.
  - There is no file lock (`:290`).
  - Then it runs a full `makakoo sync` with a 15 s timeout and ignores errors (`:293-305`).
- **`MAKAKOO_HOME`** is required: there is no fallback and it errors when unset (`:94-103`). The binary is resolved from `MAKAKOO_BIN`, then `PATH` (`:110`).
- **`LOPE_BRAIN_AUTOMEMORY=1`** writes `$MAKAKOO_HOME/data/auto-memory/lope-<name>.md` (`:342-347`).

---

## 10. Environment variables

Python Lope reads 60 `LOPE_*` / `MAKAKOO_*` variables. The full list, generated from string literals, is in [`../reference/python-lope-env-vars.txt`](../reference/python-lope-env-vars.txt). Every one is part of the parity surface unless it is explicitly dispositioned in Spec 03.
