# Spec 04 — Interfaces

[Specs](README.md) · Previous: [03 — Requirements](03-requirements-and-acceptance.md) · Next: [Sprints](../sprints/README.md)

## 1. CLI contract

**Parity baseline:** the machine-readable argparse capture [`crates/lope-compat/grammar/argparse-python-lope.json`](../../crates/lope-compat/grammar/argparse-python-lope.json) (written by `tools/parity/dump-python`) is the test oracle. The clap grammar is generated from it at build time, and A1 diffs the two (`tools/parity/{dump-python,dump-rust,diff}`). The text capture [`docs/reference/python-lope-0.17.3-cli-help.txt`](../reference/python-lope-0.17.3-cli-help.txt) is kept for people. Both are regenerated at the PL-1 tag.

Note: `--consensus` and `--structured` are two separate `store_true` actions that share the dest `consensus`, not argparse aliases. They behave the same.

| Verb | Shape (abridged; pool = shared pool flags) |
|---|---|
| `negotiate` | `GOAL [--out] [--max-rounds N] [--context] [--context-file] [--domain {engineering,business,research}] pool brain` |
| `execute` | `SPRINT_DOC [--phase] [--manual] [--gates] [--gate-config] [--trust] pool` |
| `implement` | `SPRINT_DOC [--agents] [--escalate-to] [--phase] [--gates] [--gate-config] [--trust] [--dry-run] [--interactive] pool` |
| `audit` | `SPRINT_DOC [--no-journal] pool` |
| `ask` | `QUESTION [--json] [--context] pool synth brain` |
| `review` | `FILE [--focus] [--json] [--consensus\|--structured] [--min-consensus] [--similarity] [--format {text,json,markdown,markdown-pr,sarif}] [--output/-o] [--include-raw] [--remember] [--divide {files,hunks}] [--roles] pool synth brain` |
| `vote` | `PROMPT --options OPTIONS [--json] pool synth` |
| `compare` | `FILE_A FILE_B [--criteria] [--json] pool synth` |
| `pipe` | `[--require-all] [--json] pool synth brain` (stdin) |
| `team` | `list` · `enable NAMES… [--primary]` · `disable NAMES…` · `add NAME [--cmd] [--stdin] [--url] [--model] [--key-env] [--key-header] [--key-prefix] [--response-path] [--body-json] [--from-curl] [--wrap] [--timeout] [--max-tokens] [--primary] [--disabled] [--force]` · `remove NAME` · `test NAME [PROMPT] [--timeout]` · `health [--timeout] [--json]` |
| `deliberate` | `{adr,prd,rfc,build-vs-buy,migration-plan,incident-review} SCENARIO [--depth {quick,standard,deep}] [--out DIR] [--no-anonymize] [--minority-report] [--human-questions {never,blocking,always}] [--json] pool brain` |
| `memory` | `stats [--json]` · `search QUERY [--min-score] [--limit] [--json]` · `file PATH [--limit] [--json]` · `hotspots [--days] [--limit] [--json]` · `forget [--hash] [--file]` · `gates [--limit] [--json]` |
| `gate` | `save\|check [--config] [--baseline] [--timeout] [--json] [--remember] [--trust] [--run-timeout\|--allow-unbounded-run]` |
| `check` | `[--config] [--timeout] [--json] [--remember] [--trust] [--run-timeout\|--allow-unbounded-run]` |
| `status`, `configure`, `version`, `docs` | no flags |
| `jobs` | `list [--json]` · `reap [--dry-run] [--stale] [--json]` · `prune [--stale] [--dry-run] [--json]` · `kill RUN_ID [--json]` |
| `install` | `[--host HOST]` |
| `update`, `upgrade` | `[--dry-run] [--method {auto,git,pip}] [--host] [--skip-install] [--allow-dirty]` |
| `flow` | `run WORKFLOW [--task] [--cwd] [--out] [--max-node-visits] [--no-journal] [--dry-run] pool` · `validate\|lint WORKFLOW` · `render WORKFLOW [-o] [-T]` · `init [TEMPLATE] [--out]` · `list` |

**Shared flag groups:**
- pool: `--validators --primary --timeout --respect-provider-timeout --run-timeout | --allow-unbounded-run --max-calls --max-chunks --request-policy {auto,direct,chunk} --parallel | --sequential`
- synth: `--synth --anonymous`
- brain: `--brain-context QUERY --brain-budget N --brain-log`

**Additive in v2.0:**
- top-level `--version` and `-V` (FR2)
- `review --consensus quorum` (FR23)
- `negotiate --rounds N` (FR23)
- S9 builds only: `daemon`, `pool`, `detect` (FR43)

**Exit codes:** see FR3.

## 2. Caller argv corpus (A11 fixtures)

| ID | Argv | Context | Stdout | Must hold |
|---|---|---|---|---|
| G1 | `lope version` | C4 | version text | exit 0; first `N.N` = version |
| G2 | `lope version` | C3 (launchd env) | version text | within 5 s |
| G3 | `lope jobs prune --stale` | C3 | passthrough | exit 0 on success |
| G4 | `lope review <notes.md> --consensus quorum --format markdown [--focus "<prose>"] [--brain-context q] [--remember] [--brain-log]` | C2 (stdout → `tee`) | report | `quorum` consumed; `--focus` verbatim |
| G5 | `lope review <file> --consensus --format markdown` | C3 (stdout → file) | report | SIGTERM reaps children |
| G7 | `lope negotiate "<goal>" --domain <d> --rounds 3 [--brain-context <name>] [--out p]` | C2 (cwd = worktree) | progress + result | `--rounds` alias |
| G8 | `lope implement <abs SPRINT path>` | C2 (cwd = child worktree, TTY on stdin) | interactive | roster prompt on a TTY |

Python-side callers:
- `gym_lope_gate` imports the module and never calls the CLI.
- `makakoo setup lope` runs `PYTHONPATH=~/.lope python3 -m lope version`.

Both are covered by PM-1 and PM-2.

## 3. Makakoo contracts

| Contract | Shape | Owner |
|---|---|---|
| `makakoo search <q> [-l N]` | box-drawing table or `(no hits)`; exit 0 / 1 | makakoo-os `makakoo/src/output.rs:13-38` |
| `makakoo sync [--file PATH]` | `indexed <path> as …` | `commands/sync.rs:24-28` |
| `makakoo adapter call <name> [--timeout N]` | prompt on stdin → ValidatorResult JSON | `makakoo/src/cli.rs:1834-1852` |
| Adapter registry | `~/.makakoo/adapters/registered/*.toml` | `MAKAKOO_ADAPTERS_HOME` |
| Journal | `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md` (local date), `- ` bullets, `[[links]]` | Brain conventions |
| Auto-memory | `$MAKAKOO_HOME/data/auto-memory/lope-<name>.md` | Lope |
| Optional (future) | `makakoo search --json` (PM-7); `makakoo-client` `brain_search` / `brain_write_journal` when `MAKAKOO_SOCKET_PATH` is set | makakoo-os |

## 4. Configuration

### 4.1 `~/.lope/config.json` (Python-owned format; FR6)

```json
{
  "version": 1,
  "validators": ["codex", "claude", "grok", "agy", "kimi", "claude-safe-review"],
  "primary": "codex",
  "timeout": 900,
  "parallel": true,
  "providers": [
    {
      "name": "grok",
      "type": "subprocess",
      "command": ["grok", "-p", "{prompt}"],
      "timeout": 180
    },
    {
      "name": "local-gateway",
      "type": "http",
      "url": "http://127.0.0.1:18080/v1/chat/completions",
      "headers": {"Authorization": "Bearer ${AIL_API_KEY}"},
      "body": {"model": "…", "messages": [{"role": "user", "content": "{prompt}"}]},
      "response_path": "choices.0.message.content",
      "timeout": 120
    }
  ]
}
```

- **Optional keys**, written only when not at their default: `learned_adapters, run_timeout, allow_unbounded_run, max_calls, max_input_bytes, max_output_bytes, max_chunks, request_policy`.
- **Subprocess provider fields:** `stdin, prompt_wrapper, review_command, review_workdir, review_stdin`.
- **HTTP provider fields:** `prompt_wrapper, max_tokens, response_limit`.
- No other keys may be added (DEC-3).
- The example's validators and primary match the live config's *names*. The provider entries are illustrative; never copy live values into docs (NFR13).

### 4.2 `~/.lope/super-lope/config.toml` (Rust-only; FR7; written only after PL-2)

```toml
[completion]
cleanup_budget_ms = 2000          # FR11 / Spec 01 §7
accept_with_live_descendants = false   # fail-open opt-in (OQ-6)

[routing]
force_python = false              # same as LOPE_FORCE_PYTHON=1
python = "/usr/bin/python3"       # same as LOPE_PYTHON

# S9 only (pty-pool builds)
[pool]
enabled = false
warm = []                          # provider names that opted in
idle_ttl_seconds = 1800
max_ptys = 4

[blocker]
rules_file = "~/.lope/super-lope/blocker-rules.toml"
```

## 5. State contracts

- **`~/.lope/runs/`**: `active/`, `completed/`, `work/`, `registry.lock`. Manifest schema is Python's (`jobs.py`), with forbidden keys per NFR5. Rust adds only the FR11 cleanup metadata and FR12 diagnostics fields. **Unverified:** whether Python's registry tolerates extra manifest fields. A4 must prove it before Rust writes any extra field. If it does not, the extra data goes in the `.diag` sidecar instead.
- **`run.lock`**: Python semantics (`runlock.py`).
- **`memory.db`**: [`python-lope-memory-db-schema.sql`](../reference/python-lope-memory-db-schema.sql), `user_version = 1`. Located at `$LOPE_MEMORY_DB`, else **`$HOME/.lope/memory.db`, independent of `$LOPE_HOME`** (`memory.py:44-46, 75-82`).
- **`journal/YYYY_MM_DD.md`** (audit), `journal.jsonl` (heal), `latency.json`, `trusted_gates.json`: Python formats.
- **Per-project:** `./.lope/config.json`, `./.lope/rules.json`. `.lope/reports/` belongs to Glimmer.

## 6. Verdict block

`---VERDICT---` … `---END---`. Validators emit a JSON object inside the block. The parser tries JSON first, then a YAML-ish fallback (`validators.py:4-16, 247`). Status values and repair prompts are ported verbatim from `validators.py` and `verdict_repair.py`; do not restate them here. The source is the contract.

## 7. S9 daemon protocol (deferred)

The protocol is defined at the start of S9, if OQ-4 approves S9. Fixed constraints:
- Line-delimited JSON over a Unix socket under `~/.lope/super-lope/run/`.
- Peer-UID check.
- One method set defined in one place.
- Callers fall back to T2 when the socket is absent.

## 8. Cross-repo patch sets

These ship through their own repositories. Super Lope sprints track them but never edit those repos.

| ID | Repo | Change | Needed by |
|---|---|---|---|
| **PL-1** | `~/.lope` | Commit and tag the current 0.17.3 working tree (or discard it); that tag is the parity baseline | S0 pre-flight |
| **PL-2** | `~/.lope` | Add `super-lope/` to `.gitignore` | before any write to `~/.lope/super-lope/` |
| **PL-3** | `~/.lope` | Replace `PYTHONPATH=~/.lope python3 -m lope` with `lope` in 7 SKILL.md files (`lope, lope-audit, lope-execute, lope-help, lope-negotiate, lope-team, using-lope`) and 7 `commands/` files; re-sync the 6 stale Codex copies | S8 |
| **PL-4** (optional) | `~/.lope` | Land the WIP completion fix in Python as well | anytime |
| **PL-5** | `~/.lope` | Fix `jobs.py:507-522` `is_owned_path`: the ancestor walk compares the *unresolved* path against the *resolved* root, so it never stops at the root and meets any symlinked ancestor (e.g. macOS `/var` → `/private/var`). Every owned path is then rejected, and each run with validator calls exits 1 with "owned scratch cleanup failure". Fix: walk the resolved path, or resolve the root's ancestors consistently. Found in S0 (`tools/parity` case `symlinked-lope-home-closeout-bug`). Rust must decide in S1/S2 whether to reproduce or fix it (DEC needed). | anytime; affects anyone whose `LOPE_HOME` has a symlinked ancestor |
| **PG-1** | Glimmer | `glimmer install` must not overwrite a non-shim `/usr/local/bin/lope` (`bin/glimmer:248-255`) | S8 |
| **PG-2** | Glimmer | Count manifests in `~/.lope/runs/active` instead of `state/sprints` (`bin/glimmer:193-200`, `server.js:274-281`, `edges.ts:294-310`) | S8 |
| **PG-3** | Glimmer | Show `validators` as a list (`server.js:263-269, 618-624`) | S8 |
| **PG-4** | Glimmer | Remove hardcoded `lopeVersion: '0.17.2'` (`bin/glimmer:353`) | S8 |
| **PG-5** | Glimmer | Fix `v0.17.2` pin in `watched-council.test.js:290` | S8 |
| **PG-6** | Glimmer | Capture Lope's exit code correctly under zsh (`${pipestatus[1]}` or `set -o pipefail`) in the council shell string | S8 |
| **PG-7** | Glimmer | Exclude finished jobs in `countActiveSlots` (`lib/companion/jobs.js:96-106`) | S8 |
| **PG-8** | Glimmer | Use Lope's slug rule, or `--out`, for the Plan file name (`plan-build.ts:75-96`) | S8 |
| **PM-1** | Makakoo | Migrate `gym_lope_gate` (3 copies) off `import lope.*` to a CLI contract, e.g. `lope review --json` / `lope ask --json`. Until then `~/.lope` stays importable (DEC-8) | before Python removal |
| **PM-2** | Makakoo | `makakoo setup lope`: detect a Rust `lope`, drop the `python3` requirement and the `python3 -m lope version` smoke test (`setup/lope.rs:34, 101, 206`) | S8 |
| **PM-3** | Makakoo | `MAKAKOO/AGENTS.md:100` uses `lope configure` | S8 |
| **PM-4** | Makakoo | `infect_global.py:640-645` Pi alias path check (the `~/.lope/commands/opencode` path is kept) | S8 |
| **PM-5** | Makakoo | `skill_security.rs:702` scans `~/.lope/skills` (path kept; verify only) | S8 |
| **PM-6** | Makakoo | `onboarding.py:286-313` writes `config.json` (compatible by FR6; verify only) | S8 |
| **PM-7** (optional) | Makakoo | `makakoo search --json` | anytime |
| **PM-8** | Makakoo | S9: register `loped` as a `kind="service"` plugin, or provide a launchd env per NFR16 | S9 |
| **PH-1** | host | Remove `alias lope=…` from `~/.zshrc:178` and `~/.bashrc:33` | S8 |
| **PH-2** | host | Retarget or remove `~/bin/lope` (bash wrapper that execs `python3 -m lope`) | S8 |
