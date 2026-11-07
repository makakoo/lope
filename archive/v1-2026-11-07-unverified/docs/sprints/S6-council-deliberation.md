# Sprint S6: 7-Stage Council Deliberation (`deliberate`) & Live Herdr TUI (`attach`)

[Sprints Index](README.md) · Previous: [S5 — Memory & Makakoo Bridge](S5-memory-and-makakoo-bridge.md) · Next: [S7 — Sprints & Flow Graphs](S7-sprints-and-flow-graphs.md)

---

## Goal

Implement the 7-stage adversarial Council deliberation engine (`lope deliberate` in `lope-engine::deliberate`) across all 6 templates (`adr`, `prd`, `rfc`, `build-vs-buy`, `migration-plan`, `incident-review`) and build `lope-tui` (`lope attach [<run-id>]` and `--watch`), allowing operators to watch or un-block live Council validator PTY panes in a Ratatui split-pane interface adapted from Herdr (`A13`, `SC9`, `SC10`).

## Preconditions

- `docs/evidence/s5-memory-brain.txt` exists on disk.

## Implementation Tasks

1. **7-Stage Council Deliberation Engine (`FR28`, `SC9`)**:
   - Port `lope/deliberation.py` and `lope/synthesis.py` into `lope-engine::deliberate`.
   - Implement all 6 built-in `TemplateSpec` rubrics: `adr`, `prd`, `rfc`, `build-vs-buy`, `migration-plan`, `incident-review`.
   - Execute the 7 stages deterministically:
     1. **Scenario Intake** (bounded to `48 KB`)
     2. **Independent Positions** (parallel fan-out across validators)
     3. **Anonymized Critique** (validator names stripped to `Response A`, `Response B`, `Response C`; skipped in `--depth quick`)
     4. **Revision** (validators revise/defend positions against anonymized critiques; skipped in `--depth quick`)
     5. **Primary Synthesis** (primary validator drafts unified artifact matching template sections)
     6. **Rubric Review** (each validator scores synthesis `PASS` / `NEEDS_FIX` with severity `low|medium|high`)
     7. **Minority Report** (high/critical `NEEDS_FIX` objections preserved verbatim)
   - Enforce contributor quorum (`expected / 2 + 1`) at each fan-out stage (`DeliberationInconclusive` on quorum failure).
2. **Live Split-Pane Ratatui TUI (`lope attach` & `--watch` — `FR34`, `FR35`, `SC10`)**:
   - Adapt Herdr's Ratatui pane grid renderer into `lope-tui`.
   - When `lope attach [<run-id>]` or `--watch` runs, connect to `loped` (`~/.lope/run/lope-client.sock`) and display:
     - Top status bar: active verb/stage, elapsed timer, quorum progress (`3/5 Done`), and budget usage.
     - Split-pane grid: each validator's live `ghostty-vt` screen buffer with status badge (`IDLE`, `WORKING`, `BLOCKED: <rule_id>`, `DONE`).
     - Interactive focus (`Ctrl+B` or mouse click): operator can focus a `BLOCKED` pane, type an answer, or press `Ctrl+B q` to detach without interrupting the background run.
   - Support `--machine <host>` remote SSH endpoint forwarding (`FR35`).

## Acceptance Gate & Evidence (`docs/evidence/s6-deliberate-tui.txt`)

- `A13` & `SC9` pass: All 7 deliberation stages, anonymization (`Response A/B/C`), Minority Report generation, and quorum checks pass across all 6 templates.
- `SC10` passes: Virtual terminal buffer test of `lope attach` renders split panes, status badges, and clean detach.
- Record test results in `docs/evidence/s6-deliberate-tui.txt`.

---

[Sprints Index](README.md) · Previous: [S5 — Memory & Makakoo Bridge](S5-memory-and-makakoo-bridge.md) · Next: [S7 — Sprints & Flow Graphs](S7-sprints-and-flow-graphs.md)
