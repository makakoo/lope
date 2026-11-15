# Sprint S1: PTY Execution Actor, 22-Agent Screen Detector & Darwin Reaping Fix

[Sprints Index](README.md) · Previous: [S0 — Workspace & Compat CLI](S0-workspace-and-compat-cli.md) · Next: [S2 — Warm Pool & Daemon](S2-warm-pool-and-daemon.md)

---

## Goal

Build `lope-detect` (porting Herdr's `src/detect/` engine and all 22 `.toml` agent screen manifests plus the Blocker Policy Engine) and `lope-runtime-pty` (porting Herdr's `src/pty/` actor with `ghostty-vt` screen buffering and the Leader-Completion Darwin process-group reaper), permanently eliminating defects `D1`, `D2`, and `D5` (`A3`, `A4`, `A5`).

## Preconditions

- `docs/evidence/s0-workspace.txt` exists on disk.

## Implementation Tasks

1. **`lope-detect` — 22-Agent Screen Detection Engine (`FR10`)**:
   - Port `AgentState` (`Idle`, `Working`, `Blocked`, `Unknown`), `AgentDetection`, `Agent` enum (24 variants, 22 screen manifests), and manifest evaluator from `/Users/sebastian/projects/apps/herdr/src/detect/mod.rs` and `manifest.rs`.
   - Embed all 22 `.toml` files from `/Users/sebastian/projects/apps/herdr/src/detect/manifests/*.toml` (`amp`, `antigravity`/`agy`, `claude`, `cline`, `codex`, `cursor`, `devin`, `droid`, `gemini`, `github-copilot`, `grok`, `hermes`, `kilo`, `kimi`, `kiro`, `letta`, `maki`, `muse`, `opencode`, `pi`, `qodercli`, `qwen`) via `include_str!`, with optional user overrides in `~/.lope/manifests/`.
   - Implement `lope detect <capture-file> [--agent <id>]` for instant CLI diagnostics.
2. **Blocker Policy Engine (`FR11`, `FR12` — Fixes `D2`)**:
   - Classify matched `Blocked` rules into `AutoRemediateSafe` (`startup_update`, and `trust_directory`/`trust_prompt` when `cwd` is a verified Lope scratch/workspace path), `FailFastImmediate` (`permission_prompt`, `live_strong_blocker`, `mcp_elicitation_prompt` in read-only calls, `auth_error`), and `InteractiveDelegate`.
   - Enforce `< 250ms` reaction latency and max `2` auto-remediations per call.
3. **`lope-runtime-pty` — Leader-Completion Darwin Reaper (`FR9`, `FR14`, `FR39`, `FR40` — Fixes `D1` & `D5`)**:
   - Spawn CLI validators in an isolated `portable-pty` pseudo-terminal (`120x40`) with `setsid()` process group isolation and feed output bytes into `ghostty-vt`.
   - Track leader process exit via `waitpid(leader_pid)` independently of inherited slave FDs.
   - **Darwin Fix**: When `leader_pid` exits `0` and output capture is complete, immediately return `OutcomeClass::Ok` with the extracted response. Dispatch `-pgid` cleanup (`SIGTERM` → `200ms` → `SIGKILL`) on a background task so lingering MCP/helper children never cause `cleanup_failed` or `output_drain_timeout`.
   - Enforce `2 MB` stdout / `512 KB` stderr head+tail truncation ceiling and RAII scratch-directory cleanup (`0700`).

## Acceptance Gate & Evidence (`docs/evidence/s1-pty-detector.txt`)

- `A3` passes: Mock validator that prints a valid verdict, spawns a background `sleep 30` child holding FDs open, and exits `0` returns `OutcomeClass::Ok` in `< 300ms` and reaps the child asynchronously.
- `A4` passes: All 22 agent manifest test fixtures classify `Idle`, `Working`, and `Blocked` screens with 100% accuracy.
- `A5` passes: `startup_update` and `trust_directory` are auto-remediated in `< 250ms`; read-only `live_strong_blocker` fails fast in `< 250ms`.
- Record test results in `docs/evidence/s1-pty-detector.txt`.

---

[Sprints Index](README.md) · Previous: [S0 — Workspace & Compat CLI](S0-workspace-and-compat-cli.md) · Next: [S2 — Warm Pool & Daemon](S2-warm-pool-and-daemon.md)
