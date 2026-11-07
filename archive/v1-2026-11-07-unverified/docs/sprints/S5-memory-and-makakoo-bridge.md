# Sprint S5: Rusqlite Finding Memory (`memory.db`) & Makakoo Brain Bridge

[Sprints Index](README.md) · Previous: [S4 — Single-Shot & Consensus Review](S4-single-shot-and-consensus-review.md) · Next: [S6 — Council Deliberation](S6-council-deliberation.md)

---

## Goal

Implement `lope-memory`: the persistent SQLite finding store (`~/.lope/memory.db` using `rusqlite` in WAL mode with FTS5 indexing), the `lope memory {stats,search,file,hotspots,forget}` CLI, and the Makakoo OS Brain bridge (`--brain-context <query>`, `--brain-budget <tokens>`, `--brain-log` + automatic `makakoo sync`, and `LOPE_BRAIN_AUTOMEMORY=1`) (`A11`, `A12`, `SC8`).

## Preconditions

- `docs/evidence/s4-review.txt` exists on disk.

## Implementation Tasks

1. **SQLite Finding Store (`~/.lope/memory.db` — `FR25`, `NFR10`)**:
   - Port `lope/memory.py` into `lope-memory::store` using `rusqlite` (`bundled-full`).
   - Enable `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;` so concurrent Lope reviews never lock each other out.
   - Maintain full schema compatibility with existing `~/.lope/memory.db` rows while synchronizing the `findings_fts` FTS5 index.
   - Respect `LOPE_MEMORY=off` (no-op writes/reads when set).
   - Wire `lope review --remember` to persist scored findings and implement `lope memory {stats,search,file,hotspots,forget}`.
2. **Makakoo Brain Bridge (`FR26`, `FR27`)**:
   - Port `lope/makakoo_bridge.py` into `lope-memory::makakoo`.
   - Implement `detect_makakoo()`: resolve `MAKAKOO_BIN` or `makakoo` on `PATH` and `MAKAKOO_HOME` (fallback `HARVEY_HOME` / `~/MAKAKOO`).
   - Implement `--brain-context <query>`: invoke `makakoo search <query>` via `Command::new` (`shell = false`), scrub output with `redact_text`, trim to `--brain-budget` (default `1,200` tokens / `4,800` chars), wrap in the canonical advisory markers, and prepend once to validator prompts.
   - Implement `--brain-log`: append a Logseq outliner bullet (`- [[Lope]] ...`) to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md` and run `makakoo sync` (`FR27`).
   - Implement `write_auto_memory` gated strictly behind `LOPE_BRAIN_AUTOMEMORY=1`.

## Acceptance Gate & Evidence (`docs/evidence/s5-memory-brain.txt`)

- `A11` passes: `lope review --remember` and `lope memory {stats,search,file,hotspots,forget}` pass, including a 4-writer concurrent WAL stress test.
- `A12` & `SC8` pass: `--brain-context` injects redacted context once, and `--brain-log` appends to the journal and runs `makakoo sync`.
- Record test results in `docs/evidence/s5-memory-brain.txt`.

---

[Sprints Index](README.md) · Previous: [S4 — Single-Shot & Consensus Review](S4-single-shot-and-consensus-review.md) · Next: [S6 — Council Deliberation](S6-council-deliberation.md)
