# S2 — Supervised runtime and completion policy

[Sprints](README.md) · Previous: [S1](S1-core-config-and-state.md) · Next: [S3](S3-providers-and-team.md)

## Goal

Build Tier 2 (DEC-2): one-shot CLI validators over pipes with the fail-closed completion policy (DEC-4, Spec 01 §7). This addresses D1 and D5 without weakening Python's safety contract, and instruments D2 (FR12).

## Preconditions

`docs/evidence/s1-core-state.txt` exists.

## Tasks

1. **`lope-runtime`:**
   - spawn with argv only, `setsid`, and prompt delivery (stdin then close, argv, or 0600 file);
   - async bounded head+tail capture (2 MiB / 512 KiB); argv limit 128 KiB;
   - deadlines with `--respect-provider-timeout` semantics;
   - `LOPE_RUN_ID`, `LOPE_CALL_ID` and `LOPE_PROCESS_MARKER` exported to children (FR10).
2. **Completion policy** (FR11): kill-then-confirm group cleanup, cleanup budget (default 2.0 s, configurable), `UNVERIFIED` metadata, and `LOPE_RECOVER_UNVERIFIED_OUTPUT`.
3. **Timeout diagnostics** (FR12) in `.diag` sidecars; manifest fields only if S1 proved tolerance.
4. **Signals** (FR13) and RAII temp resources (FR14). Run deadline with exit 124 (FR9).
5. `platform::{macos,linux}` only (NFR11). No sleep-polling apart from post-exit group confirmation (NFR3).

## Acceptance

- A5, A6 and A7 pass, and A5 is repeated 50× on the Intel Mac with zero false `UNVERIFIED`.
- Measure the group-confirmation latency distribution on the Intel Mac and record it. It justifies the default cleanup budget.
- Evidence: `docs/evidence/s2-runtime.txt`.

## Status (2026-11-07)

**Closed.** Evidence: [`docs/evidence/s2-runtime.txt`](../evidence/s2-runtime.txt). A5 (50×, zero false UNVERIFIED), A6 and A7 pass; parent death kills the provider group; latency distribution recorded. Deviations: the supervisor is a separate process (the `lope` binary under argv0 `lope-supervisor`) to keep Python's owner-SIGKILL safety; `.diag` files live in `runs/diag/` because Python's closeout requires empty work dirs; FR9/FR13 adoption by verbs happens in S4.
