# S6 — Council deliberation

[Sprints](README.md) · Previous: [S5](S5-makakoo-bridge.md) · Next: [S7](S7-sprint-engine-and-flow.md)

## Goal

Port `deliberate` exactly (FR31).

## Preconditions

`docs/evidence/s5-makakoo.txt` exists.

## Tasks

1. Port `deliberation.py`, `ensemble.py` and `synthesis.py`:
   - the positional template and scenario (`-` = stdin);
   - the seven stages, with `quick` skipping critique and revision;
   - quorum `expected // 2 + 1`; labels `Response A…AA`;
   - 48/32/64 KB limits and 5 workers;
   - `--no-anonymize`, `--minority-report`, `--human-questions` (accepted; Python does not consume it either), `--json`;
   - output to `--out DIR` or `lope-runs/<ts>-<template>/` (`LOPE_RUN_OUTPUT_DIR`).
2. Mine `tests/test_deliberation.py` and `test_synthesis.py` for goldens.

## Acceptance

- A14 passes.
- Evidence: `docs/evidence/s6-deliberate.txt`.
