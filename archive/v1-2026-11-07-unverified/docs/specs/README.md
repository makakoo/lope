# Super Lope Product & Technical Specifications

[Root Index](../../README.md) · [Audits](../audits/README.md) · **Specs** · [Sprints](../sprints/README.md)

This directory is the **single canonical specification set** for **Super Lope** (`lope` v2.0 in Rust). It unifies the product rules, Cargo crate workspace architecture, 3-tier execution engine, use cases, verification scenarios, functional and non-functional requirements, acceptance tests, and wire/CLI protocols into four cross-linked documents.

---

## Specification Map

| # | Document | What It Defines | Primary Backlinks |
|---|---|---|---|
| 1 | [01 — Product, Architecture & Security](01-product-and-architecture.md) | Product definition, fusion matrix (`Lope + Herdr + Makakoo`), 10-crate Cargo workspace topology, Runtime Contexts (`C1`–`C6`), 3-Tier Execution Engine (`HTTP` / `One-Shot PTY` / `Warm Agent Pool`), Blocker Policy Engine, Leader-Completion Reaping Rule, and Security/Privacy boundaries | [Lope Audit](../audits/01-lope-python-audit.md), [Herdr Audit](../audits/02-herdr-rust-audit.md), [S0](../sprints/S0-workspace-and-compat-cli.md)–[S2](../sprints/S2-warm-pool-and-daemon.md) |
| 2 | [02 — Use Cases & User Scenarios](02-use-cases-and-scenarios.md) | Formal use cases (`UC1`–`UC18`), end-to-end verification scenarios (`SC1`–`SC15`), and the 4 Operator Journeys (CLI One-Shot, Glimmer Desktop Council, Autonomous Sprint/Flow, Live Attached Herdr TUI) | [Requirements](03-requirements-and-acceptance.md), [Sprints S0–S8](../sprints/README.md) |
| 3 | [03 — Requirements & Acceptance Tests](03-requirements-and-acceptance.md) | Functional requirements (`FR1`–`FR40`), non-functional requirements (`NFR1`–`NFR15`), explicit v2.0 non-goals (`L1`–`L8`), automated tests (`A1`–`A16`), and manual tests (`M1`–`M8`) | [Use Cases](02-use-cases-and-scenarios.md), [Interfaces](04-interfaces-and-protocol.md), [Traceability Matrix](../sprints/README.md#traceability-matrix) |
| 4 | [04 — Interfaces, Protocol & Schemas](04-interfaces-and-protocol.md) | Complete CLI `argv` & exit-code contract, `~/.lope/config.json` & `lope.toml` schemas, `lope.socket.v1` JSON-RPC daemon protocol, `---VERDICT---` & consensus parsers, SQLite `memory.db` schema, DOT flow grammar, and SARIF 2.1.0 output | [Seams Audit](../audits/03-makakoo-and-glimmer-seams-audit.md), [S0](../sprints/S0-workspace-and-compat-cli.md), [S3](../sprints/S3-validators-and-http-engine.md)–[S8](../sprints/S8-glimmer-cutover-and-benchmarks.md) |

---

## Canonical Identifier Registry

Every sprint in [`docs/sprints/`](../sprints/README.md) and [`SPRINT-SUPER-LOPE-V1.md`](../../SPRINT-SUPER-LOPE-V1.md) references these exact IDs:

| Prefix | Range | Meaning | Defined In |
|---|---|---|---|
| `D` | `D1`–`D5` | Verified root-cause defect in Python Lope `0.17.3` | [Audits Index](../audits/README.md#verified-root-cause-defect-matrix-why-python-lope-fails--how-super-lope-fixes-it) |
| `C` | `C1`–`C6` | Runtime execution context governing process, PTY, and daemon behavior | [01 — Product, Architecture & Security](01-product-and-architecture.md#runtime-contexts-c1c6) |
| `UC` | `UC1`–`UC18` | Formal use case (preconditions, main flow, blocker handling, postconditions) | [02 — Use Cases & User Scenarios](02-use-cases-and-scenarios.md#use-cases-uc1uc18) |
| `SC` | `SC1`–`SC15` | Deterministic end-to-end verification scenario | [02 — Use Cases & User Scenarios](02-use-cases-and-scenarios.md#verification-scenarios-sc1sc15) |
| `FR` | `FR1`–`FR40` | Testable functional requirement | [03 — Requirements & Acceptance Tests](03-requirements-and-acceptance.md#1-functional-requirements-fr1fr40) |
| `NFR` | `NFR1`–`NFR15` | Non-functional constraint (performance, memory, Darwin reaping, security) | [03 — Requirements & Acceptance Tests](03-requirements-and-acceptance.md#2-non-functional-requirements-nfr1nfr15) |
| `L` | `L1`–`L8` | Explicit non-goals deferred beyond v2.0 | [03 — Requirements & Acceptance Tests](03-requirements-and-acceptance.md#3-later-on-purpose-explicit-v20-non-goals-l1l8) |
| `A` | `A1`–`A16` | Automated acceptance test suite | [03 — Requirements & Acceptance Tests](03-requirements-and-acceptance.md#4-automated-acceptance-tests-a1a16) |
| `M` | `M1`–`M8` | Manual acceptance check (live CLI, Glimmer, TUI attach) | [03 — Requirements & Acceptance Tests](03-requirements-and-acceptance.md#5-manual-acceptance-tests-m1m8) |
| `S` | `S0`–`S8` | Serial sprint slice gated by `docs/evidence/` outputs | [Sprints Index](../sprints/README.md) |

---

## Core Product Rules (Non-Negotiable)

1. **100% Drop-In Replacement for `/usr/local/bin/lope` (`FR1`–`FR4`)**: Glimmer (`makakoo-jungle`), Makakoo OS (`makakoo`), and all 17 installed Lope agent skills must work without modifying a single line of caller code. Existing `~/.lope/config.json` and `~/.lope/memory.db` files are read natively.
2. **Real PTY + Ghostty-VT + 22-Agent Screen Detection (`FR10`–`FR12`)**: CLI validators never run inside blind `subprocess.PIPE` black boxes. They execute inside `portable-pty` + `ghostty-vt` virtual terminals inspected by Herdr's 22 declarative `.toml` manifests (`claude`, `codex`, `gemini`, `agy`, `grok`, `opencode`, `kimi`, `qwen`, `pi`, etc.).
3. **Leader-Completion Authority on macOS (`FR9`, fixes `D1` & `D5`)**: Once a validator's leader PID exits `0` and its response is complete, the call succeeds immediately. Background child helpers (MCP servers, git watchers) in the process group are cleaned up asynchronously and never cause a completed answer to be discarded as `cleanup_failed`.
4. **3-Tier Execution Engine (`FR13`–`FR15`)**:
   - **Tier 1 (`Native HTTP`)**: Direct `reqwest` async HTTP calls for API/gateway providers (`0` subprocesses).
   - **Tier 2 (`Managed One-Shot PTY`)**: Ephemeral PTY execution with `< 250ms` blocker detection and safe auto-remediation.
   - **Tier 3 (`Warm Agent Pool`)**: Persistent warm agent panes managed by `loped` (`~/.lope/run/lope.sock`) for zero cold-boot overhead across multi-stage deliberations and sprints.
5. **Headless by Default, Visual on Demand (`FR28`)**: Every `lope` CLI command runs headlessly and streams clean output to stdout/files just like v0.17, while `lope attach [<run-id>]` opens Herdr's Ratatui split-pane TUI to watch or intervene in live validator panes in real time.

---

[Root Index](../../README.md) · Next: [01 — Product, Architecture & Security](01-product-and-architecture.md)
