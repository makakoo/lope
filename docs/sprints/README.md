# Super Lope — Sprint roadmap (S0–S9)

[Root](../../README.md) · [Master sprint](../../SPRINT-SUPER-LOPE-V1.md) · [Audits](../audits/README.md) · [Specs](../specs/README.md) · **Sprints**

Plan revision 2 is a **strangler** sequence (DEC-1):
- From S0, the Rust binary routes every invocation, executing natively what is ported and delegating the rest to Python Lope.
- Each sprint grows the native coverage table and proves parity against Python before an entry is added.
- `/usr/local/bin/lope` changes once, in S8.
- S9 is optional and gated on evidence.

| Sprint | Document | Native after the sprint | Evidence |
|---|---|---|---|
| **S0** | [Pre-flight, workspace, router](S0-preflight-workspace-and-router.md) | `version` (+ `--version`, `-V`); argv normalisation + nesting guard for all | `pins.txt`, `s0-router.txt` |
| **S1** | [Core, config, state](S1-core-config-and-state.md) | `status`, `jobs *` | `s1-core-state.txt` |
| **S2** | [Supervised runtime & completion policy](S2-supervised-runtime-and-completion.md) | — (library) | `s2-runtime.txt` |
| **S3** | [Providers & team](S3-providers-and-team.md) | `team *`, `configure` | `s3-providers.txt` |
| **S4** | [Single-shot, review, memory](S4-single-shot-review-and-memory.md) | `ask`, `vote`, `compare`, `pipe`, `review`, `memory *` (without brain flags) | `s4-review-memory.txt` |
| **S5** | [Makakoo bridge](S5-makakoo-bridge.md) | brain flags on all verbs that have them | `s5-makakoo.txt` |
| **S6** | [Council deliberation](S6-council-deliberation.md) | `deliberate` | `s6-deliberate.txt` |
| **S7** | [Sprint engine & flow](S7-sprint-engine-and-flow.md) | `negotiate`, `execute`, `implement`, `audit`, `gate *`, `check`, `flow *` (code landed `0cc251b`; parity gaps fixed in S8B) | `s7-sprint-flow.txt` |
| **S8** | [Cutover](S8-cutover.md) | `install`, `docs` (`update`/`upgrade` per OQ-2; closeout & `A17`/`M1–M8` in S8B) | `s8-cutover.txt` |
| **S8B** | [Remediation & cutover closeout](S8B-remediation-and-cutover-closeout.md) | S7/S8 parity fixes, `FR39` docs, `PH`/`PL`/`PG`/`PM` patches, `A17`, `M1–M8` | `s7-sprint-flow.txt`, `s8-cutover.txt` |
| **S9** | [PTY warm pool & daemon (gated)](S9-pty-pool-and-daemon.md) | `daemon`, `pool`, `detect` in `pty-pool` builds | `s9-pool.txt` |

## Execution rules

1. **Serial.** S(n) starts only when `docs/evidence/` holds S(n−1)'s evidence file, and that file records the coverage-table hash plus the A-tests passed.
2. **Parity before coverage.** A verb or flag set enters the coverage table only when its golden tests pass against Python Lope (NFR14).
3. **Rollback at every boundary.** At each sprint end, run the NFR6 check: Python Lope reads everything Rust wrote.
4. **No edits to other repos.** Cross-repo patches (PL/PG/PM/PH, [Spec 04 §8](../specs/04-interfaces-and-protocol.md#8-cross-repo-patch-sets)) are tracked here and executed in their own repos.
5. **Evidence hygiene** (NFR13). Record outputs and timings, never secrets or `config.json` values.

## Traceability

| Sprint | FR | NFR | Tests |
|---|---|---|---|
| S0 | FR1, FR2, FR4, FR5, FR23 (router) | NFR1, NFR2 (startup), NFR14 | A1, A2 |
| S1 | FR6, FR7, FR8, FR9 (lock), FR25 (parser), FR26 (redaction) | NFR5, NFR6, NFR7, NFR8 | A3, A4 |
| S2 | FR9 (deadline), FR10–FR14 | NFR3, NFR4, NFR11 | A5, A6, A7 |
| S3 | FR15–FR20 | NFR4, NFR10 | A8 |
| S4 | FR3, FR21, FR22, FR24, FR25, FR26, FR27 | NFR2 (RSS), NFR8, NFR9 | A9, A10, A11 (G4, G5), A12 |
| S5 | FR28–FR30 | NFR5 | A13 |
| S6 | FR31 | NFR10 | A14 |
| S7 | FR32–FR36 | NFR4 | A11 (G7, G8), A15, A16 |
| S8 | FR37–FR39 | NFR6, NFR12, NFR13, NFR15 | A17, M1–M8 |
| S8B | FR4, FR32–FR37, FR39 | NFR4, NFR6, NFR12, NFR13, NFR15 | A11 (G7, G8), A15, A16, A17, M1–M8 |
| S9 | FR40–FR43 | NFR16 | A18 |
