# S5 — Makakoo bridge

[Sprints](README.md) · Previous: [S4](S4-single-shot-review-and-memory.md) · Next: [S6](S6-council-deliberation.md)

## Goal

Make the brain flags native with the DEC-10 hygiene fixes, and prepare the Makakoo-side patch set.

## Preconditions

`docs/evidence/s4-review-memory.txt` exists.

## Tasks

1. **`lope-makakoo`:**
   - `--brain-context`: `makakoo search`, ANSI and box stripping, `(no hits)` treated as empty, redaction, budget, prepended once (FR28);
   - `--brain-log`: `MAKAKOO_HOME` required, Python's bullet text, local date, exclusive lock, `makakoo sync --file` (FR29);
   - `LOPE_BRAIN_AUTOMEMORY` (FR30).
2. Add brain flags to the coverage table for `negotiate` (still delegated as a verb until S7), `ask`, `review`, `pipe` and `deliberate` (verb delegated until S6). Flags only become native for verbs that are native.
3. **Write the Makakoo patch descriptions** (PM-1…PM-6) with exact file:line and proposed diffs, as issues or PR drafts for makakoo-os. Do not edit makakoo-os from this repo.
4. Confirm `gym_lope_gate` still runs against `~/.lope` (DEC-8) by running its dry-run path.

## Acceptance

- A13 passes; the `gym_lope_gate` dry run is green.
- Evidence: `docs/evidence/s5-makakoo.txt`, including the PM patch list status.
