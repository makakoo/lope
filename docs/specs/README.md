# Super Lope — Specifications

[Root](../../README.md) · [Audits](../audits/README.md) · **Specs** · [Sprints](../sprints/README.md)

Plan revision 2. These four documents define what Super Lope must do. Every behaviour labelled *parity* is defined by Python Lope as captured in [`docs/reference/`](../reference/). Behaviour beyond parity is labelled *additive* or *deviation* and traced to a decision below.

| # | Document | Defines |
|---|---|---|
| 1 | [01 — Product & architecture](01-product-and-architecture.md) | Goals, runtime contexts, crate layout, execution tiers, strangler shim, completion policy, Herdr reuse, security |
| 2 | [02 — Use cases](02-use-cases-and-scenarios.md) | UC1–UC17 tied to real callers |
| 3 | [03 — Requirements & acceptance](03-requirements-and-acceptance.md) | FR1–FR43, NFR1–NFR16, L1–L10, A1–A18, M1–M8 |
| 4 | [04 — Interfaces](04-interfaces-and-protocol.md) | CLI parity contract, caller argv corpus, config/state/schema contracts, Makakoo contracts, cross-repo patch sets |

## ID registry

| Prefix | Range | Meaning | Defined in |
|---|---|---|---|
| `D` | D1–D7 | Defects, with verification status | [Audits README](../audits/README.md#defect-matrix) |
| `DEC` | DEC-1–DEC-11 | Design decisions (revisable) | below |
| `OQ` | OQ-1–OQ-6 | Open questions needing an owner decision | below |
| `C` | C1–C7 | Runtime contexts | [01 §2](01-product-and-architecture.md#2-runtime-contexts-c1c7) |
| `UC` | UC1–UC17 | Use cases | [02](02-use-cases-and-scenarios.md) |
| `FR` | FR1–FR43 | Functional requirements | [03 §1](03-requirements-and-acceptance.md#1-functional-requirements) |
| `NFR` | NFR1–NFR16 | Non-functional requirements | [03 §2](03-requirements-and-acceptance.md#2-non-functional-requirements) |
| `L` | L1–L10 | Non-goals for v2.0 | [03 §3](03-requirements-and-acceptance.md#3-non-goals-for-v20) |
| `A` | A1–A18 | Automated acceptance tests (scenarios are folded in) | [03 §4](03-requirements-and-acceptance.md#4-automated-acceptance-tests) |
| `M` | M1–M8 | Manual acceptance checks | [03 §5](03-requirements-and-acceptance.md#5-manual-acceptance-checks) |
| `PL`/`PG`/`PM`/`PH` | — | Cross-repo patch sets (Python Lope / Glimmer / Makakoo / host shell) | [04 §8](04-interfaces-and-protocol.md#8-cross-repo-patch-sets) |
| `S` | S0–S9 | Serial sprints | [Sprints](../sprints/README.md) |

## Decisions

Each decision exists because a verified fact forced a choice. They can be revisited; changing one means updating the FRs it lists.

| ID | Decision | Forced by | Affects |
|---|---|---|---|
| **DEC-1** | **Strangler cutover.** The Rust `lope` routes each invocation natively only when its verb *and* flag set are ported; otherwise it execs Python Lope with argv, env, cwd, stdio and exit status preserved. | Revision 1 left Glimmer-critical verbs non-functional until S4/S7 and cut everything over in one step. | FR5, A2, all sprints |
| **DEC-2** | **One-shot CLI validators run over pipes**, not a PTY: stdin delivered then closed, separate stdout/stderr. A PTY is used only by the opt-in warm pool (S9). | Herdr manifests are interactive-only, and unmatched means Idle (Audit 02 §2). A PTY merges streams, wraps or mangles JSON, and changes CLI behaviour. claude and codex read stdin to EOF. | FR10, FR14, FR41 |
| **DEC-3** | `config.json` stays in Python's format: `"version": 1`, Python's keys only, written with Python's lock and rename semantics. Rust-only settings live in `~/.lope/super-lope/config.toml`. | Python ignores configs without `version: 1` and drops unknown keys on save (`config.py:80, 394-433`). Makakoo `onboarding.py` also writes `config.json`. | FR6, FR7, NFR6 |
| **DEC-4** | **Completion policy stays fail-closed by default** (Python WIP semantics). Kill the process group, then confirm it is empty within a longer asynchronous budget. If descendants survive, mark the call `UNVERIFIED` with metadata; output is returned only under `LOPE_RECOVER_UNVERIFIED_OUTPUT=1`. Accepting answers despite live descendants is an explicit opt-in. | The WIP deliberately fails closed (`output.py:54`, CHANGELOG). Revision 1's "always accept" silently reversed a safety policy. | FR11 |
| **DEC-5** | `ghostty-vt`, `portable-pty` and Herdr detection sit behind cargo feature `pty-pool`, **off by default**. The default build needs only cargo. | `ghostty-vt` needs Zig 0.16 and a network fetch (Audit 02 §1). | NFR1, S9 |
| **DEC-6** | State paths and schemas are unchanged: `~/.lope/runs/`, `memory.db` (same tables, `user_version = 1`, `LIKE` search), `run.lock`, `journal/`, `latency.json`, `trusted_gates.json`. | Rollback to Python at any sprint boundary (NFR6). | FR8, FR9, FR27 |
| **DEC-7** | **Drop revision-1 L3** ("no Glimmer changes"). Glimmer gets a small patch set (PG-1…PG-8). | `glimmer install` overwrites `/usr/local/bin/lope` with the Python shim; Glimmer reads a nonexistent manifest dir. | Spec 04 §8 |
| **DEC-8** | `~/.lope` (the Python checkout) stays installed and importable through v2.0. | `gym_lope_gate` imports `lope.*`; rollback target. | PM-1, NFR6 |
| **DEC-9** | v2.0 defers remote SSH (`--machine`), the `lope attach` TUI, FTS in `memory.db`, and short role-lens aliases. | Herdr remote needs Herdr on the host; its UI is not extractable; FTS endangers rollback; Glimmer sends prose. | L5–L8 |
| **DEC-10** | **Deviation:** `--brain-log` uses the **local** date, takes a file lock, and runs `makakoo sync --file <journal>`. `--brain-context` strips ANSI and treats `(no hits)` as empty. | Python uses the UTC date, takes no lock, runs a full sync, and injects `(no hits)` and box-drawing text (Audit 03 §2.2). | FR28, FR29 |
| **DEC-11** | **Additive Glimmer compatibility:** `review --consensus quorum` (≡ bare `--consensus`), `negotiate --rounds N` (≡ `--max-rounds`), `lope --version` and `-V`. The shim rewrites these before delegating, so Glimmer works even for verbs not yet ported. | D4, D6. | FR2, FR23 |

## Open questions (need Sebastian)

| ID | Question | Default until answered |
|---|---|---|
| **OQ-1** | Meaning of `--consensus unanimous` and `--consensus any`. Nothing in Python or Glimmer defines them. | Rejected with exit 2; only `quorum` is accepted. |
| **OQ-2** | Distribution: does the Rust binary replace Python in `traylinx/lope`, or ship from a new repo or Homebrew formula? What happens to the pip package `lope-agent` and to `lope update --method git\|pip`? | `update`/`upgrade` stay delegated to Python until decided. |
| **OQ-3** | Version string: `2.0.0` or continue `0.18.x`? | `lope 2.0.0 (super-lope; compat 0.17.3)` |
| **OQ-4** | Should S9 (PTY warm pool, blocker policy, daemon) happen at all? | Decided by FR12 timeout diagnostics from at least 7 days of dogfooding native `review`/`ask` (available from S4). |
| **OQ-5** | Is the DEC-10 journal deviation (UTC → local date) acceptable? | Adopt local date. |
| **OQ-6** | DEC-4: does any caller want fail-open completion by default? | Fail-closed. |
