# S8 — Cutover

[Sprints](README.md) · Previous: [S7](S7-sprint-engine-and-flow.md) · Next: [S9](S9-pty-pool-and-daemon.md)

## Goal

Make the Rust binary the `lope` that every caller reaches, with a one-step rollback, after the cross-repo patches have landed.

## Preconditions

- `docs/evidence/s7-sprint-flow.txt` exists.
- OQ-2 is decided, or `update`/`upgrade` stay delegated.
- OQ-3 is decided (version string).

## Tasks

1. **`install`** (FR37) producing the same host artifacts as `~/.lope/install` for all 8 hosts; **`docs`** (FR39); `update`/`upgrade` per OQ-2 (FR38).
2. **Cross-repo patches land in their own repos:** PL-3 (skills and commands), PG-1…PG-8 (Glimmer), PM-2…PM-6 (Makakoo). PM-1 can stay open, because `~/.lope` remains importable (DEC-8).
3. **Release build** on the Intel Mac.
   - Copy the current `/usr/local/bin/lope` shim to `/usr/local/bin/lope-py` and verify `lope-py version`.
   - Install the Rust binary at `/usr/local/bin/lope`.
4. **Host shell (PH-1, PH-2):** remove the aliases and retarget `~/bin/lope`. Confirm that `type lope` in a fresh zsh resolves to the Rust binary.
5. **Verification:**
   - A17;
   - M1–M8, with benchmarks Rust vs `lope-py` for startup, router overhead, RSS and review wall-clock;
   - Glimmer suite vs the NFR15 baseline;
   - the `gym_lope_gate` nightly run (M5).
6. Log the cutover in the Makakoo Brain journal and run `makakoo sync --file`.

## Rollback

`ln -sf /usr/local/bin/lope-py /usr/local/bin/lope`, or `LOPE_FORCE_PYTHON=1`. Restore the aliases only if needed. No state migration is required (NFR6).

## Acceptance

- A17 and M1–M8 pass.
- Evidence: `docs/evidence/s8-cutover.txt` with the benchmark table, the Glimmer baseline diff, `glimmer doctor` output and the patch-set status.
