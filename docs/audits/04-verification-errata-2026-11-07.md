# Audit 04 — Verification errata for plan revision 1 (2026-11-07)

[Audits](README.md) · Previous: [03 — Seams](03-makakoo-and-glimmer-seams-audit.md)

Every material claim in [`archive/v1-2026-11-07-unverified/`](../../archive/v1-2026-11-07-unverified/) is dispositioned below. Evidence is in Audits 01–03 unless cited inline.

**Legend:**
- ✅ verified
- ⚠️ partly right
- ❌ wrong
- ➕ presented as parity but actually new scope
- ❓ unproven

**Where corrected:** the revision-2 location that now carries the fix.

## A. Pins and metrics

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| Python Lope `0.17.3 @ 9cffa55` | ⚠️ | HEAD is 0.17.2; 0.17.3 exists only as uncommitted changes in 15 files | Audits README, PL-1 |
| 61 `.py` files (53 + 8) | ❌ | 62 (53 + 9; `flow/__init__.py` was miscounted) | Audit 01 §1 |
| 27,322 LOC | ✅ | — | — |
| Herdr `0.9.5 @ ee5bfb50`, Apache-2.0, `src/` 291,081 LOC | ✅ | — | — |
| "291,081 LOC in `src/` + `crates/ghostty-vt`" | ⚠️ | `src/` alone is 291,081; ghostty-vt adds 12,331 | README |
| 22 manifests, 2,186 lines | ⚠️ | 22 ✅; 1,798 lines ❌ | Audit 02 §1 |
| `manifest.rs` 1,465 LOC | ❌ | 1,536 | Audit 02 §2 |
| Herdr workspace members `[".", "crates/ghostty-vt"]` | ⚠️ | `members = ["crates/ghostty-vt"]`; the root package is in `default-members` | Audit 02 §1 |
| Makakoo `0.4.0 @ 2e1bfe7`, 6 crates, MIT | ✅ | — | — |
| makakoo reqwest `["json","rustls-tls","stream"]` | ⚠️ | also `multipart`, and **`default-features = false`** | Audit 03 §2.5 |
| Glimmer `8a541055`, `/usr/local/bin/glimmer` | ✅ | symlink into `makakoo-jungle/bin` | — |
| Hardware i5-8257U / 16 GB / `/usr/local` | ✅ | — | — |
| "21 CLI verbs" (README) / numbered list in Audit 01 | ❌ | 23 verbs; the list numbering skipped entries | Audit 01 §2 |
| "12 installed skills" | ❌ | 17 (the same docs also said 17) | Audit 03 §3 |

## B. Defects (D1–D5)

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| D1 root cause at `supervisor.py:102-118` in 0.17.3 @ 9cffa55 | ❌ | The Darwin `return 1` is an **uncommitted** addition; HEAD runs `ps` on Darwin | Defect matrix |
| "20/21 abandoned `cleanup_failed` manifests" | ❓ | Not reproducible: 0/139 `descendants_live`, 4 `inspection_unknown`, 0 active | Defect matrix |
| "`processes.py:314` discards stdout" | ⚠️ | It raises `ProcessCompletionError`. The WIP keeps stdout on the error, reports `UNVERIFIED`, and offers opt-in recovery: a deliberate fail-closed design | FR11, DEC-4 |
| "Leader-Completion Authority" as a plain bug fix | ⚠️ | It reverses Python's deliberate fail-closed policy for live descendants, so it is a **policy decision** | DEC-4 |
| D2: trust/update prompts cause 120–900 s blind hangs | ❓ | Stdin is closed after input; claude `--print` and codex `exec` are non-interactive; no captured prompt. 27 timeouts with unknown cause | FR12, S9 gate |
| D3: double interpreter + 20 ms poll | ✅ | — | — |
| D3: `ThreadPoolExecutor(max_workers=5)` | ⚠️ | Only in `deliberation.py:712`, with variable max | — |
| D3 baseline "~180–350 ms" Python startup | ❌ | 1.00–1.25 s measured | Defect matrix |
| D4 `--version` unrecognised | ✅ | — | — |
| D5 inherited-FD drain timeout | ⚠️ | The path exists, but the group is SIGKILLed before draining; 0 live occurrences | Defect matrix |

## C. CLI and compatibility

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| Global `-c/--config`, `--machine`, `--no-daemon`, `--watch` | ➕ | Don't exist. `-c/--config` would **collide** with `check`/`gate --config` | Spec 04 §1 |
| `--respect-provider-timeout` is global | ❌ | It's a per-subcommand flag in the pool group | Spec 04 §1 |
| `review --consensus [quorum\|unanimous\|any]` is parity | ➕ | Python's is `store_true` (alias `--structured`). Glimmer's `--consensus quorum` already exits 2 | FR23 |
| `review` flag list | ⚠️ | Omits `--json --min-consensus --similarity -o --divide --roles --synth --anonymous`, the pool flags and `--structured` | Spec 04 §1 |
| 7 role lenses `--focus product\|sales\|tax\|…` | ➕ | `--focus` is free text plus minimality aliases; role lenses live under `--roles` with a different set. Glimmer sends prose | FR22, L8 |
| `negotiate --rounds 1..5` | ❌ | Only `--max-rounds` (no upper bound). Glimmer's `--rounds` already exits 2 | FR23 |
| `implement --primary --escalate --max-retries` | ❌ | Real flags are `--agents --escalate-to --phase --gates --gate-config --trust --dry-run --interactive` | Spec 04 §1 |
| `execute --max-retries` | ❌ | Fixed at 3 | Audit 01 §2 |
| `deliberate --template`, `--output` | ❌ | The template is positional; output is `--out DIR` | Spec 04 §1 |
| `ask --markdown` | ❌ | Doesn't exist | — |
| `compare --criteria` mandatory | ❌ | Optional, with a default | — |
| `jobs {list, show, prune --stale, reconcile}` | ❌ | `{list, reap, prune, kill}` | Spec 04 §1 |
| `flow {run, validate, render, templates}` | ❌ | `{run, validate/lint, render, init, list}` | Spec 04 §1 |
| `team {…}` | ⚠️ | Also has `health` | — |
| `memory {…}` | ⚠️ | Also has `gates` | — |
| `configure` is flag-driven | ❌ | Interactive only | — |
| Exit 3 = Blocked with `--fail-on-blocked` | ❌ | 3 = untrusted gates / `--manual` without a TTY; there is no such flag | FR3 |
| Exit codes 0/1/2/3/130 complete | ❌ | Missing 75 (run lock) and 124 (deadline), plus install passthrough | FR3 |
| `lope version` → `lope 2.0.0 (…)` matches the doctor's "`>= 0.17.3` semver check" | ⚠️ | The doctor takes the first `N.N` with no minimum; 2.0.0 passes | FR2 |
| `install` hosts include Antigravity / `~/.gemini/skills` | ❌ | Hosts are `claude codex gemini opencode cursor vibe qwen pi`; gemini also gets `commands/lope` | FR37 |
| `--interactive-blockers`, `--fail-on-blocked` | ➕ | Undefined anywhere | removed |

## D. State, config and schemas

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| Manifests in `~/.lope/state/sprints/` | ❌ | `~/.lope/runs/{active,completed,work}`; Glimmer carries the same bug | FR8, PG-2 |
| `_FORBIDDEN_MANIFEST_KEYS` at `jobs.py:37-48` | ✅ | Real list has 10 keys | NFR5 |
| `status` prints `N manifest(s): active=X, abandoned=Y` and Glimmer parses it | ⚠️ | The format is roughly right; **Glimmer never runs `status`** | Audit 03 §1.1 |
| config.json example (no `version`; `pool`, `blocker_policy`, `max_parallel_ptys`) | ❌ | Python ignores configs without `"version": 1` **and drops unknown keys on save** | FR6, FR7, DEC-3 |
| memory.db `findings(target_file, line_start, validators_json, fingerprint, run_id…)` + `runs` | ❌ | Real schema differs completely; no `runs` table | FR27, reference SQL |
| Add FTS5 `findings_fts` (no triggers) | ➕ | Would need triggers or a rebuild; risks the `user_version` rollback guard | L7 |
| `PRAGMA busy_timeout=5000` present | ⚠️ | WAL + `synchronous=NORMAL` are set; busy handling is the 5 s connect timeout | NFR9 |
| `LOPE_IMPLEMENTATION_DEPTH` max 2 | ❌ | Any depth > 0 → every command exits 2 | FR4 |
| Audit journal `~/.lope/journal.jsonl` | ❌ | `$LOPE_HOME/journal/YYYY_MM_DD.md`, on by default; `journal.jsonl` is heal events | FR34 |
| Flow interpolation `{{node_id.output}}` | ❌ | `$task`/`$VAR` and `{node.key}` | FR36 |
| Flow node types (no `gate`) | ⚠️ | Includes `gate` plus aliases | FR36 |

## E. Validators and runtime

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| 15 built-ins incl. grok, kimi, cursor, copilot | ❌ | 5 classes + 10 presets; grok and kimi are user providers here | FR15 |
| `makakoo_adapter.py` is a switchAILocal adapter | ❌ | It is `makakoo adapter call` validators | FR17 |
| Headroom runtime boundaries in Lope | ❌ | Installer and skill only | Audit 01 §1 |
| Separate 2 MB stdout / 512 KB stderr under a PTY | ❌ | Values ✅ for pipes; a PTY merges the streams | DEC-2 |

## F. Makakoo and Glimmer seams

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| Glimmer doctor runs under `env -i …` | ❌ | Inherits `process.env`; `env -i` appears only in Glimmer's spec | Audit 03 §1.1 |
| Glimmer reads `lope status` | ❌ | Never runs it | — |
| Glimmer passes `--output`; Lope writes `council-<id>.md` | ❌ | `tee` of stdout writes it | FR22 |
| `scripts/fake-lope` works via a PATH override | ❌ | Called by absolute path | — |
| Glimmer FR16 = Plan | ❌ | FR17 | — |
| `NFR15` "Glimmer A1–A15 green" | ❌ | The baseline already fails (v0.17.2 pin, zsh PIPESTATUS, slot leak) | NFR15 |
| L3 "no Glimmer changes needed" | ❌ | `glimmer install` rewrites `/usr/local/bin/lope` to the Python shim | DEC-7, PG-1 |
| "All 17 skills work with zero edits" | ❌ | 7 SKILL.md, 7 commands and 6 Codex copies use `python3 -m lope` | PL-3 |
| Makakoo integration = `--brain-context` / `--brain-log` only | ❌ | Misses gym gate imports, `setup lope`, onboarding, adapters, infect, skill scan, AGENTS.md | Audit 03 §2.1 |
| `makakoo search` output suitable for injection | ❌ | Box table, 80-char snippets, `(no hits)` | FR28 |
| Read-only SQLite FTS fallback | ➕ | No such fallback in Python; it would bind to an unversioned, migrating schema | removed |
| `MAKAKOO_HOME` fallback `$HARVEY_HOME` / `~/MAKAKOO` | ❌ | Python has no fallback; Makakoo falls back to `HARVEY_HOME` → `~/.makakoo` | FR29 |
| Journal date (implicitly local) | ⚠️ | Python uses UTC; Makakoo uses local | FR29, DEC-10 |
| `com.makakoo.lope` label is free | ✅ | — | — |
| launchd daemon needs no env work | ❌ | Validators live in `~/.local/bin` and nvm; keys come from `~/.env` | NFR16 |

## G. Herdr

| Rev-1 claim | Verdict | Finding | Corrected in |
|---|---|---|---|
| ghostty-vt is a pure-Rust VT crate; copy it plus portable-pty | ❌ | A binding to Zig-built libghostty-vt; Zig 0.16 + network fetch; S0's copy list breaks the build | DEC-5, S9 |
| "Single static binary, zero runtime deps" | ⚠️ | Static linking of libghostty is fine; macOS never allows fully static binaries (libSystem) | NFR1 |
| `startup_update` in claude and grok; claude trust rules; `auth_error`; `rate_limit_modal` | ❌ | Don't exist | FR42 |
| "Every manifest encodes blocked rules" | ❌ | pi has none | Audit 02 §2 |
| Detection works for `claude --print` / `codex exec` under a PTY | ❌ | Manifests are interactive-only; unmatched → Idle | DEC-2 |
| Detector alone yields Idle/Working/Blocked | ⚠️ | Working comes from the pane-layer stabiliser | FR41 |
| `SubmitUserInput {…, deadline, …}` | ⚠️ | No `deadline` on Unix | Audit 02 §3 |
| Herdr verifies peer UID (`LOCAL_PEERCRED`) | ❌ | Only 0600 file mode | FR40 |
| Herdr kills process groups | ❌ | Per-pid HUP → TERM → KILL over the session | Audit 02 §3 |
| A4 reuses Herdr fixtures | ❌ | None exist; Herdr's `AGENTS.md:201` forbids such tests | FR42 |
| `lope attach` = extract Herdr's Ratatui grid | ❌ | Server-side compositing; a rewrite | L6 |
| SSH hosts `traylinx-ci` / `hz` via Herdr | ❌ | Not in Herdr; remote mode needs Herdr on the host | L5 |
| NFR12 "preserve Apache-2.0 headers for ghostty-vt, portable-pty" | ❌ | No headers exist; portable-pty and libghostty-vt are MIT | NFR12 |
| "Clean-room" Rust fusion | ❌ | The plan copies Herdr code | README, NFR12 |

## H. Internal inconsistencies removed

- RSS target 25 MB vs 30 MB → one value (NFR2).
- Startup target 15 ms (UC1) vs 10 ms → one value.
- Default max PTYs 5 vs 4 → S9 only, one value.
- SC14 owned by S1 and S2 → scenarios folded into A/M tests.
- FR38 socket methods vs Spec 04 method list → socket protocol deferred to S9 with a single definition.
- `lope.toml` vs `config.toml` → one file, DEC-3.
- `lope-client.sock` referenced but undefined → removed (L6).
- Warm pool placed in `lope-tui` while `lope-providers` routes into it, breaking the "no cycles" DAG → new crate layout (Spec 01 §3).
