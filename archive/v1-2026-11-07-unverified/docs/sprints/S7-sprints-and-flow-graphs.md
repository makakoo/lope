# Sprint S7: Sprint Engine (`negotiate`/`execute`/`implement`/`audit`) & Declarative Flow Graphs (`flow`)

[Sprints Index](README.md) · Previous: [S6 — Council Deliberation](S6-council-deliberation.md) · Next: [S8 — Glimmer Cutover & Benchmarks](S8-glimmer-cutover-and-benchmarks.md)

---

## Goal

Implement the multi-phase sprint lifecycle (`negotiate --domain`, `execute`, `implement`, `audit`, `gate`, `check`) in `lope-engine::sprint` and the declarative Graphviz DOT workflow engine (`lope flow {run,validate,render,templates}`) in `lope-flow` (`A14`, `A15`, `SC11`, `SC12`, `SC13`).

## Preconditions

- `docs/evidence/s6-deliberate-tui.txt` exists on disk.

## Implementation Tasks

1. **Multi-Round Sprint Negotiation (`lope negotiate` — `FR29`, `SC11`)**:
   - Port `lope/negotiator.py` and `lope/models.py` (`SprintDoc`, `Phase`, `DOMAINS`) into `lope-engine::sprint::negotiate`.
   - Support `--domain {engineering,business,research}` configuring domain-specific phase rubrics:
     - `engineering`: `Goal`, `Criteria`, `Files`, `Tests`
     - `business`: `Goal`, `Criteria`, `Deliverables`, `Success Metrics`
     - `research`: `Goal`, `Criteria`, `Artifacts`, `Validation Criteria`
   - Run primary draft → validator ensemble critique → revision rounds (`--rounds`) → write `SPRINT-<SLUG>.md` (or `--out <path>`).
2. **Sprint Execution, Swarm Implement, Gates & Audit (`FR30`–`FR32`, `SC12`)**:
   - Port `lope/executor.py`, `lope/implement.py`, `lope/healer.py`, `lope/gates.py`, and `lope/auditor.py`.
   - **`lope execute <sprint-doc>`**: For each phase, run primary implementer → run objective verification gates (`lope gate`) → run validator ensemble review (`---VERDICT---`) → retry/heal on `NEEDS_FIX` up to `--max-retries` → advance on `PASS`.
   - **`lope implement <sprint-doc>`**: Zero-human swarm execution with primary + escalation roster, incrementing `LOPE_IMPLEMENTATION_DEPTH` (max depth `2`).
   - **`lope audit <sprint-doc>`**: Generate execution scorecard (`PASS`/`NEEDS_FIX`/`FAIL`, confidence, duration) and append to `~/.lope/journal.jsonl` when requested.
3. **Declarative Graphviz DOT Flow Engine (`lope flow` — `FR33`, `SC13`)**:
   - Port `lope/flow/{dot,model,validate,runner,stylesheet,templates,render,report}.py` into crate `lope-flow`.
   - Parse DOT digraphs with `start`, `exit`, `agent`, `review`, `judge`, and `script` nodes plus conditional edges (`outcome == "PASS"`, `outcome == "NEEDS_FIX"`).
   - Execute waves concurrently (`JoinSet`), gate barrier fan-in nodes until all non-loop predecessors complete, interpolate `Blackboard` variables (`{{node_id.output}}`), and strictly enforce per-node `max_visits` and graph-wide `max_node_visits` (counting retry attempts toward the cap).

## Acceptance Gate & Evidence (`docs/evidence/s7-sprints-flow.txt`)

- `A14`, `SC11` & `SC12` pass: `negotiate --domain business`, `implement` with `NEEDS_FIX -> PASS` retry healing, `gate`, and `audit` pass.
- `A15` & `SC13` pass: `lope flow validate`, `lope flow render`, and `lope flow run` execute fan-out/fan-in and terminate cyclical graphs at `max_node_visits`.
- Record test results in `docs/evidence/s7-sprints-flow.txt`.

---

[Sprints Index](README.md) · Previous: [S6 — Council Deliberation](S6-council-deliberation.md) · Next: [S8 — Glimmer Cutover & Benchmarks](S8-glimmer-cutover-and-benchmarks.md)
