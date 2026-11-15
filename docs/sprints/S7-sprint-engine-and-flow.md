# S7 — Sprint engine and flow

[Sprints](README.md) · Previous: [S6](S6-council-deliberation.md) · Next: [S8](S8-cutover.md)

## Goal

Port `negotiate`, `execute`, `implement`, `audit`, `gate`, `check`, the healer, and `flow` (FR32–FR36). This covers Glimmer G7 and G8.

## Preconditions

`docs/evidence/s6-deliberate.txt` exists.

## Tasks

1. **`negotiate`:**
   - `--max-rounds` (with `--rounds` alias via the router), `--context`, `--context-file`, `--domain`, `--out`;
   - domain sections; Python's slug rule; `.rounds` sidecar; `LOPE_NEGOTIATE_*` knobs.
2. **`execute`** (3 retries per phase, `--manual` exit 3 without a TTY) and **`implement`**:
   - real flags; interactive roster prompt on a TTY;
   - `LOPE_IMPLEMENTATION_DEPTH` export to writers;
   - `LOPE_EVIDENCE_GATE`, `LOPE_YES_RUN_PROJECT_COMMANDS`.
3. **`audit`** (journal on by default), **`gate save|check`**, **`check`**: `.lope/rules.json`, trust store, exit 3. **Healer** and the heal journal.
4. **`flow`:**
   - node types and aliases;
   - `$task`, `$VAR` and `{node.key}` interpolation;
   - caps and retries;
   - templates; `run`, `validate/lint`, `render`, `init`, `list`.
5. Mine `tests/test_implement*.py`, `test_execute_gates.py`, `test_gates.py`, `test_gate_trust.py`, `test_negotiate_*.py`, `test_flow.py`, `test_self_heal.py`, `test_journal.py`.

## Acceptance

- A11 (G7, G8), A15 and A16 pass.
- With this sprint, every verb except `install`, `docs`, `update` and `upgrade` is native.
- Evidence: `docs/evidence/s7-sprint-flow.txt`.
