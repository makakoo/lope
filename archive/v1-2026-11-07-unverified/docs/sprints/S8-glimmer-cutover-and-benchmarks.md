# Sprint S8: Glimmer (`makakoo-jungle`) Cutover, Skill Sync & Intel Mac Benchmarks

[Sprints Index](README.md) · Previous: [S7 — Sprints & Flow Graphs](S7-sprints-and-flow-graphs.md) · [Master Sprint](../../SPRINT-SUPER-LOPE-V1.md)

---

## Goal

Compile the optimized release binary (`cargo build --release`), preserve `/usr/local/bin/lope-py` as the legacy Python fallback shim, atomically install the Rust `lope` binary to `/usr/local/bin/lope`, update host skills (`lope install`), run full end-to-end compatibility verification with **Glimmer (`makakoo-jungle`)** and **Makakoo OS**, and record the Intel Mac latency and memory benchmark comparison (`A16`, `SC1`, `SC7`, `SC15`, `M1`–`M8`).

## Preconditions

- `docs/evidence/s7-sprints-flow.txt` exists on disk.

## Implementation Tasks

1. **Release Build & Atomic Binary Cutover (`FR4`, `NFR1`)**:
   - Build `/Users/sebastian/Projects/apps/super-lope/target/release/lope` with `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`.
   - Copy the existing `/usr/local/bin/lope` Python shim to `/usr/local/bin/lope-py` and verify `lope-py version` still invokes `~/.lope` (`0.17.3`).
   - Install `/Users/sebastian/Projects/apps/super-lope/target/release/lope` to `/usr/local/bin/lope` and start `lope daemon start` (`com.makakoo.lope` launchd agent).
2. **Host Skill Installer & Built-In Docs (`FR36`, `FR37`)**:
   - Implement `lope docs` and `lope install`, refreshing the skill definitions in `/Users/sebastian/.gemini/skills/lope*` and `~/.claude/skills/lope*`.
3. **Glimmer (`makakoo-jungle`) End-to-End Verification (`A16`, `NFR15`, `M5`, `M6`)**:
   - Run `env -i HOME="$HOME" PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin" makakoo version && lope version && lope --version`.
   - Run `glimmer doctor` and verify `Makakoo OS`, `Brain FTS`, and `Lope` all report `✓` ready.
   - Run Glimmer's companion and watched Council test suite in `/Users/sebastian/Projects/apps/makakoo-jungle` against the new `/usr/local/bin/lope` binary.
4. **Intel Mac Performance & Resource Benchmark (`SC15`, `NFR2`)**:
   - Measure and record side-by-side metrics on the dogfood Intel Mac (`i5-8257U`) comparing `/usr/local/bin/lope-py` (Python `0.17.3`) vs. `/usr/local/bin/lope` (Rust `2.0.0`):
     - Cold CLI startup (`lope version`): target `< 10 ms` (vs. `~180–350 ms` in Python).
     - Peak RSS during 5-validator parallel fan-out: target `< 25 MB` (vs. `~150–250 MB` across Python parent + 5 `lope.supervisor` interpreters).
     - Blocker detection time on simulated `trust_directory` prompt: `< 250 ms` (vs. `900,000 ms` timeout in Python).
     - Darwin lingering-child completion rate: `100%` `Ok` (vs. `cleanup_failed` in Python).

## Acceptance Gate & Evidence (`docs/evidence/s8-cutover-bench.txt`)

- `A16`, `SC1`, `SC7`, `SC15`, and `M1`–`M8` pass 100%.
- Write the `glimmer doctor` output, test suite summary, and the Python-vs-Rust benchmark table into `docs/evidence/s8-cutover-bench.txt`.
- Log the completion summary to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md` and run `makakoo sync`.

---

[Sprints Index](README.md) · Previous: [S7 — Sprints & Flow Graphs](S7-sprints-and-flow-graphs.md) · [Root Index](../../README.md)
