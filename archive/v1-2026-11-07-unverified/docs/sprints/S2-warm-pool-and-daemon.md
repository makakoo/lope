# Sprint S2: Background Daemon (`loped`), Warm Agent Pool & Run Registry

[Sprints Index](README.md) · Previous: [S1 — PTY Supervisor & Detector](S1-pty-supervisor-and-detector.md) · Next: [S3 — Validators & HTTP Engine](S3-validators-and-http-engine.md)

---

## Goal

Implement the privacy-safe run manifest registry (`~/.lope/state/sprints/` + `lope jobs prune --stale`), the `loped` background daemon (`lope.socket.v1` at `~/.lope/run/lope.sock` with `LOCAL_PEERCRED` peer UID verification), and the **Tier 3 Persistent Warm Agent Pool** (`lope pool {list,warm,drain,reset}`) (`A6`, `SC5`, `SC14`).

## Preconditions

- `docs/evidence/s1-pty-detector.txt` exists on disk.

## Implementation Tasks

1. **Run Manifest Registry & Stale Lease Pruning (`FR6`, `NFR6`, `SC14`)**:
   - Port `RunRegistry` from `lope/jobs.py` into `lope-core::jobs`: write privacy-safe manifests in `~/.lope/state/sprints/`, enforcing `_FORBIDDEN_MANIFEST_KEYS` (`prompt`, `response`, `argv`, `env` never stored).
   - Implement `process_start_fingerprint(pid)` (`ps -p <pid> -o lstart=` on macOS, `/proc/<pid>/stat` on Linux) and `boot_fingerprint()`.
   - Implement `lope status` and `lope jobs {list,show,prune --stale,reconcile}` so dead/abandoned manifests are safely archived/pruned while live runs are never touched.
2. **Background Daemon `loped` & `lope.socket.v1` (`FR5`, `FR38`, `NFR3`)**:
   - Implement `lope daemon {start,stop,restart,status}` modeled on Herdr's `src/server/headless.rs`.
   - Bind `~/.lope/run/lope.sock` (`0600` inside `0700` dir) and verify `peer_uid == geteuid()` via `LOCAL_PEERCRED` / `getpeereid`.
   - Expose JSON-RPC 2.0 methods (`daemon.ping`, `daemon.status`, `daemon.shutdown`, `pool.list`, `pool.warm`, `pool.drain`, `pool.reset`, `pool.lease_and_prompt`, `run.list`, `run.subscribe_events`).
3. **Tier 3 Persistent Warm Agent Pool (`FR15`, `SC5`)**:
   - Maintain warm interactive PTY panes in `loped` for configured agents (`claude`, `codex`, `gemini`, `opencode`).
   - Verify `AgentState::Idle` via `lope-detect` before marking a pane `Ready`.
   - On `pool.lease_and_prompt`, submit the prompt via `PtyIoActorHandle::SubmitUserInput`, subscribe to `Idle -> Working -> Idle` (or `Blocked`) state transitions, extract the response turn from `ghostty-vt`, and reset pane state (`/clear` or `/reset`) before returning the pane to the pool.
   - When `loped` is not running (`C1` context), callers fall back transparently to Tier 2 (`One-Shot PTY`) with zero errors.

## Acceptance Gate & Evidence (`docs/evidence/s2-daemon-pool.txt`)

- `A6` & `SC5` pass: `loped` starts with `< 15 MB` idle RSS, rejects unauthorized socket peers, warms 2 mock interactive agent panes, and executes 4 consecutive requests through the warm panes in `< 50ms` dispatch overhead each.
- `SC14` passes: `lope jobs prune --stale` prunes 3 dead-PID manifests and preserves 1 live-PID manifest.
- Record test output and daemon RSS in `docs/evidence/s2-daemon-pool.txt`.

---

[Sprints Index](README.md) · Previous: [S1 — PTY Supervisor & Detector](S1-pty-supervisor-and-detector.md) · Next: [S3 — Validators & HTTP Engine](S3-validators-and-http-engine.md)
