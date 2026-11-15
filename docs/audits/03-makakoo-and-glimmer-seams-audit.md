# Audit 03 — Makakoo OS, Glimmer and shell seams

[Audits](README.md) · Previous: [02 — Herdr](02-herdr-rust-audit.md) · Next: [04 — Errata](04-verification-errata-2026-11-07.md)

Verified on 2026-11-07 against:

- Glimmer: `/Users/sebastian/Projects/apps/makakoo-jungle` @ `8a541055`
- Makakoo OS source: `/Users/sebastian/projects/apps/makakoo-os` @ `2e1bfe7` (0.4.0)
- Makakoo runtime home: `/Users/sebastian/MAKAKOO`
- Shell configuration on the dogfood Mac

This audit lists **every live caller of `lope`** and every Lope-owned file those callers touch. Anything not listed here is not a dependency.

---

## 1. Glimmer (`makakoo-jungle`)

### 1.1 Call sites

| # | Caller | Exact argv | How it runs |
|---|---|---|---|
| G1 | `bin/glimmer:134` (`glimmer doctor`) | `lope version` | `${prefix}/bin/lope`, where prefix is `/opt/homebrew` on arm64 if present, else `/usr/local` (`:30-35, :120`). Inherits `process.env`; there is **no `env -i`**. 10 s timeout. |
| G2 | `lib/companion/server.js:248` (health) | `lope version` | Same binary, launchd env, 5 s timeout. |
| G3 | `server.js:546` (`jobs.archiveStale`) | `lope jobs prune --stale` | Only exit 0 is checked; stdout is passed through. |
| G4 | `src/main/glimmer/council-runner.ts:79-94`, launched from `EditorPanelHeader.tsx:235`, `ReviewNotesSendMenuContent.tsx:258`, `GrabConfirmationSheet.tsx:207` | `lope review <.lope/reports/council-<id>-notes.md> --consensus quorum --format markdown [--focus <prose>] [--brain-context q] [--remember] [--brain-log]` | PTY tab running the shell string `lope '…' \| tee <report>; echo "${PIPESTATUS[0]:-$?}" > <.exit>`. |
| G5 | `bin/glimmer:635, :714` → `server.js:778` (headless) | `lope review <file> --consensus --format markdown` | Detached, no shell. Stdout goes to `.jungle/reviews/<id>.md` and stderr to a log. Env adds `PYTHONPATH=$LOPE_HOME`. |
| G6 | `bin/glimmer:624` | (prints `lope review "<abs>" --consensus --format markdown`) | Text only; never run. |
| G7 | `src/main/glimmer/plan-build.ts:135-142` → `QuickOpen.tsx:455` | `lope negotiate <goal> --domain <d> --rounds 3 [--brain-context <worktree>] [--out p]` | PTY tab; cwd is the worktree. |
| G8 | `plan-build.ts:354` → `QuickOpen.tsx:502` | `lope implement <abs-sprint-path>` (no flags) | PTY tab; cwd is the child worktree. |

Glimmer never runs `lope status`, `deliberate`, `ask`, `memory`, `install`, `--version` or `-V`. It never sets `LOPE_IMPLEMENTATION_DEPTH`. It injects `GLIMMER_SOCKET` and `JUNGLE_SOCKET` into PTYs (`pty-host-env.ts:68-71`), but Lope ignores both.

**Already broken against Python Lope 0.17.3:**

- G4: `--consensus` is `store_true`, so argparse rejects the leftover `quorum` and exits 2.
- G7: `--rounds` is not a defined flag (Python uses `--max-rounds`), so argparse exits 2.

### 1.2 Version check

- The parser is the first match of `/v?([0-9]+\.[0-9]+(?:\.[0-9]+)?)/` on stdout, with exit 0 required (`bin/glimmer:136`, `server.js:250`). There is **no minimum version** in code; Glimmer's spec says 0.16+.
- With Python's output, it captures `0.17.3` from `sprint runner v0.17.3`. With `lope 2.0.0 (super-lope; compat 0.17.3)` it captures `2.0.0`, which passes.
- `lopeVersion: '0.17.2'` is hardcoded elsewhere (`bin/glimmer:353`).

### 1.3 Lope files Glimmer reads directly

| Path | Where | What it reads |
|---|---|---|
| `~/.lope/config.json` | `server.js:263-269, 618-624` | `primary_model \|\| primary`, `Object.keys(validators)`, `timeout`, `parallel`. `validators` is a **list**, so Glimmer shows indices "0".."5". |
| `~/.lope/state/sprints/*.json` | `bin/glimmer:193-200`, `server.js:274-281`, `edges.ts:294-310` | File count only. **The directory doesn't exist** (Python uses `~/.lope/runs/`), so the count is always 0. |
| `~/.lope` git state | `bin/glimmer:126-130, 233-241` | `git status --porcelain` and `rev-parse`. A dirty tree is reported, and `glimmer install` refuses to run. |
| `$LOPE_HOME` | shim | used as `PYTHONPATH` |

### 1.4 Report and stdout contract

- Glimmer **never passes `--output`.** The Council report `.lope/reports/council-<id>.md` is written by `tee` from Lope's stdout. Glimmer then strips ANSI codes and rewrites the file (`exit-watcher.ts:63-66`). The notification body is the first non-`#` line.
- The headless path writes `.jungle/reviews/<id>.md` from stdout (`server.js:727, 782`).
- **Stdout is therefore the report.** Python's review header lines on stdout are tolerated. Progress must stay on stderr, as it already is in Python (`progress.py:29`).
- Under zsh (the user's shell), `${PIPESTATUS[0]:-$?}` records `tee`'s exit code, so Lope's exit code is lost. This is a pre-existing Glimmer bug.

### 1.5 Lens strings, slugs, cancel

- `--focus` strings are long prose such as `"tax-fiscal: statutory citations, tax residency, …"`. Engineering passes no `--focus` at all. The definitions are duplicated at `EditorPanelHeader.tsx:48-61` and `council-branch.ts:17-30`. **They must pass through verbatim.**
- Glimmer predicts the sprint file name with its own `SPRINT-<UPPER>.md` slug (`plan-build.ts:75-96`), which disagrees with Python's 60-character slug on long goals. Build falls back to the first `SPRINT-*.md`.
- Cancel sends SIGTERM to the Lope PID only, not its process group (`server.js:516`).

### 1.6 Glimmer tests and specs: baseline that is already not green

- `tests/s3/watched-council.test.js:290` requires `v0.17.2` in the version output. It already fails on 0.17.3.
- That test also requires `lope review --help` to list `--consensus --format --focus --brain-context` with exit 0 (`:298-303`).
- `scripts/fake-lope` handles `review <x>` by printing `# council`, a blank line and `ok`, then exits 0; anything else exits 2. It is called by absolute path (`watched-council.test.js:267`), not through a `PATH` override.
- There is a slot leak: `countActiveSlots` counts finished jobs (`lib/companion/jobs.js:96-106`), so after two runs M-tests report "busy" whatever Lope does.
- **Glimmer spec numbering, as cited by super-lope v1:**
  - FR9, FR10, FR17–18, FR34 and FR38: correct.
  - FR16: wrong. FR16 is Design Mode; Plan is FR17.
  - Glimmer spec 03:105 is the only place that uses `env -i`.
- **Shim:** `glimmer install` writes `/usr/local/bin/lope` as `#!/bin/sh … exec /usr/bin/python3 -m lope "$@"` (`bin/glimmer:248-255`). Re-running it would overwrite a Rust binary.

### 1.7 Launchd

- `com.makakoo.jungle` is written by `glimmer install` (`bin/glimmer:303`), but it is not loaded or installed on this machine.
- `edges.ts:333` repairs `com.makakoo.glimmer-companion.plist`.
- The companion socket is named `companion.sock` in code, but `jungle.sock` is what exists on disk.

---

## 2. Makakoo OS

### 2.1 Integration points (every one; none are in super-lope v1)

| # | Integration | Evidence | Contract |
|---|---|---|---|
| M1 | **SANCHO task `gym_lope_gate`** (every 84,600 s, active hours 3–6) | `plugins-core/mascot-gym/plugin.toml:33`; `makakoo-core/src/sancho/mod.rs:399` | **Imports Python Lope**: `sys.path.insert(0, "~/.lope")` then `from lope.validators import build_validator_pool, VerdictStatus`, `lope.config.load_layered`, `lope.negotiator._build_validator_prompt`, `lope.models.Proposal`. Three copies: `makakoo-os/plugins-core/lib-harvey-core/src/core/gym/lope_gate.py:85-89`, `makakoo-os/plugins-core/mascot-gym/src/gym/lope_gate.py:80-84`, `MAKAKOO/plugins/lib-harvey-core/src/core/gym/lope_gate.py:85-86`. |
| M2 | `makakoo setup lope` | `makakoo/src/commands/setup/lope.rs` | Treats Lope as installed if `~/.lope/lope/cli.py` exists (`:34`). Requires `git`, `python3` and `bash` (`:101`). Clones `traylinx/lope` (`:13-15`) and runs `bash ~/.lope/install`. Smoke test is `PYTHONPATH=~/.lope python3 -m lope version`. Prints an alias hint (`:206`). |
| M3 | Makakoo adapters as Lope validators | `makakoo/src/cli.rs:1834-1852, 1937-1945`; `lope/makakoo_adapter.py` | `makakoo adapter call <name> [--timeout N]` returns ValidatorResult JSON. Registered on this Mac: `pi`, `switchailocal`, `tytus-cli`. |
| M4 | Host onboarding | `lib-harvey-core/src/core/orchestration/onboarding.py:286-313`; `MAKAKOO/config/cli_registry.json:9` | Appends to `validators` in `~/.lope/config.json`. |
| M5 | Python `infect` (Pi CLI) | `infect_global.py:640-645` | `alias pi=… --skill ~/.lope/commands/opencode` |
| M6 | Skill security scan | `makakoo-core/src/skill_security.rs:702` | Scans `~/.lope/skills`. |
| M7 | Agent instructions | `MAKAKOO/AGENTS.md:90-101` (line 100: `PYTHONPATH=~/.lope python3 -m lope configure`) | — |
| M8 | MCP `pattern_lope_question` | `plugins-core/pattern-lope-question/plugin.toml` | An LLM rewrite of a question. **Does not run `lope`.** |

### 2.2 Brain search and sync

- `makakoo search <QUERY> [-l N]` (`makakoo/src/cli.rs:33-39`) prints a **box-drawing table** (doc_id, source, type, score, snippet cut to 80 characters) or a styled `(no hits)` (`output.rs:13-38`). There is no `--json`. Python Lope injects that text into prompts as-is, including `(no hits)`.
- The Brain has several sources, including Obsidian and `~/.codex/memories` (`MAKAKOO/config/brain_sources.json`).
- `makakoo sync [--force] [--embed] [--no-auto-memory] [--embed-limit N] [--file PATH]` (`cli.rs:88-108`). `--file` re-indexes one file.
- **Index:** `$MAKAKOO_HOME/data/superbrain.db`, table `brain_fts(name, content, entities)` with `content=brain_docs`, porter tokenizer (`makakoo-core/src/db.rs:183-188`), WAL. Ranking is bm25 plus a boost (`store.rs:566-583`). Live size is 88 MB with a 359 MB WAL. Opening the store runs migrations (`store.rs:351-353`), so a direct read-only SQLite "fallback" would bind to an internal, unversioned schema.
- **Journal:**
  - Makakoo uses the **local** date (`makakoo-mcp/src/handlers/tier_b/journal.rs:34-40`). Python Lope uses UTC.
  - Locked write paths exist: MCP `brain_write_journal` (lock, read-modify-write, index) and kernel capability `brain.write_journal` (`capability/service/brain.rs:127-134`). There is no `makakoo remember` command.

### 2.3 `makakoo-client`

`makakoo-client/src/lib.rs` is a small crate (tokio, serde, thiserror, base64). It exposes `brain_search` (JSON hits, `:361`), `brain_write_journal` (`:418`), `llm_chat`, `state_*` and `secret_read`.

- **Constraint:** it requires `$MAKAKOO_SOCKET_PATH`, which exists only for processes launched by `makakoo skill <name>` (`commands/skill.rs:76-92`). A `lope` run from a shell has no socket.
- `makakoo-core` already defines `ValidatorResult`, `PhaseVerdict` and a `---VERDICT---` parser (`makakoo-core/src/adapter/result.rs:46-104`).

### 2.4 Services, environment, plugins

- **Launchd labels:**
  - Makakoo's own daemons are `com.makakoo.daemon` (`makakoo-platform/src/macos.rs:17`) and `com.makakoo.agent.<slot>` (`agents/launchd.rs:25`).
  - About 30 `com.makakoo.*` plists exist in `~/Library/LaunchAgents`.
  - `com.makakoo.lope` is unused.
- **Service environment:**
  - Makakoo's daemon plist wraps the command as `sh -c 'set -a; . ~/.env; set +a; exec …'` and sets `MAKAKOO_HOME`, `HARVEY_HOME` and a fixed `PATH` (`macos.rs:41, 67-74`).
  - Agent plists put a validated node directory first in `PATH` (`launchd.rs:249-264`, `service_env.rs:21-34`).
  - On this Mac, `claude` and `vibe` live in `~/.local/bin`, while `gemini`, `qwen` and `node` exist only under `~/.nvm/versions/node/*/bin`. A bare launchd environment cannot start them.
- **Plugins:**
  - Manifest format is `plugin.toml` (`spec/PLUGIN_MANIFEST.md`), with `kind="service"` handing start/stop/health to a binary (precedent: `plugins-core/garage-store/plugin.toml:4, 25-31`).
  - Capability grants: `spec/CAPABILITIES.md`.
- **`MAKAKOO_HOME` resolution in Makakoo:** `MAKAKOO_HOME`, then `HARVEY_HOME`, then `~/.makakoo` on macOS (`makakoo-core/src/platform.rs:24-41`). Python Lope has no fallback.

### 2.5 Dependency alignment (`makakoo-os/Cargo.toml`)

Edition 2021, `tokio 1.40 (full)`, `rusqlite 0.32 (bundled-full)`, `reqwest 0.12 (default-features = false; json, rustls-tls, multipart, stream)`, `clap 4 (derive)`, `crossterm 0.28`, `blake3 1.5`, `sha2 0.10`, `regex 1.11`, `toml 0.8`, `axum 0.7`.

- Herdr uses crossterm 0.29. A conflict only matters if TUI types are shared.
- `default-features = false` on reqwest is required to avoid pulling in native TLS.

---

## 3. Skills and shells

- **17 Lope skills:** `using-lope` plus 16 `lope*`. They are symlinked from `~/.lope/skills` into gemini, claude, agents, opencode (skills and `.md` commands), `MAKAKOO/skills-shared`, qwen and vibe.
  - Codex holds 17 real **copies**, already out of sync with the source.
  - Cursor has 4 (lope, lope-audit, lope-execute, lope-negotiate).
  - Gemini also has `commands/lope/*.toml`.
- **Python-style invocations** (`PYTHONPATH=~/.lope python3 -m lope …`) appear in 7 SKILL.md files (`lope`, `lope-audit`, `lope-execute`, `lope-help`, `lope-negotiate`, `lope-team`, `using-lope`), 7 files under `~/.lope/commands`, 6 Codex copies and `MAKAKOO/AGENTS.md:100`.
- **Shell precedence:** `alias lope='PYTHONPATH=/Users/sebastian/.lope python3 -m lope'` in `~/.zshrc:178` and `~/.bashrc:33` wins in interactive shells. `~/bin/lope` (bash wrapper → `python3 -m lope`) precedes `/usr/local/bin` on `PATH`. A binary at `/usr/local/bin/lope` is reached only by GUI and launchd callers and by shells without these.
- `MAKAKOO/bootstrap/global.md` and `MAKAKOO/CLAUDE.md` do not mention Lope.
