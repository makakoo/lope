# Audit 02 — Herdr

[Audits](README.md) · Previous: [01 — Python Lope](01-lope-python-audit.md) · Next: [03 — Seams](03-makakoo-and-glimmer-seams-audit.md)

Verified against `/Users/sebastian/projects/apps/herdr` on 2026-11-07: branch `master` @ `ee5bfb50`, package `herdr 0.9.5`, edition 2021, Apache-2.0 (`LICENSE`).

**Bottom line:** Herdr is a terminal multiplexer for *interactive* agent TUIs. Its detection engine is reusable with modest stubbing. Its PTY actor, server, remote and UI layers are tightly coupled to `AppState` and are better re-implemented than extracted. Nothing in Herdr helps one-shot `--print`/`exec` validators, which is how Lope invokes every built-in CLI today.

---

## 1. Workspace and build

- `Cargo.toml`: root package plus `[workspace] members = ["crates/ghostty-vt"]`, `default-members = [".", "crates/ghostty-vt"]`.
  - `[patch.crates-io] portable-pty = { path = "vendor/portable-pty" }` (`:57-58`).
  - Deps: tokio 1 (resolves 1.50), ratatui 0.30, crossterm 0.29, interprocess 2.4.2, bincode 2, toml 0.8, toml_edit 0.22, jsonc-parser 0.33.1, schemars 1.2.1, clap 4.5, regex 1, sha2 0.10, tracing 0.1.44.
  - No rusqlite and no reqwest; Herdr shells out to curl.
- `rust-toolchain.toml`: channel **1.96.1**. On the dogfood Mac, Homebrew `cargo 1.89.0` comes first on `PATH`, ignores the pin, and rustup has no 1.96.1 installed.
- **`crates/ghostty-vt` is not pure Rust.** The crate describes itself as "Herdr's binding to its vendored libghostty-vt" (`crates/ghostty-vt/Cargo.toml`).
  - `build.rs:72-116` runs `zig build -Demit-lib-vt` in `../../vendor/libghostty-vt`. That requires **Zig 0.16.0** (`build.rs:105,114`; `build.zig.zon:6`), and Zig is not installed here.
  - The Zig build **fetches `uucode` from the network** (`build.zig.zon:49-53`), unless `LIBGHOSTTY_VT_ZIG_SYSTEM_DIR` is supplied (Nix path, `build.rs:93`, `nix/package.nix`).
  - The output is a static `libghostty-vt.a`, so the final binary is self-contained, but the build is not cargo-only.
  - `build.rs:37-38` hardcodes the `../../vendor` sibling layout. Reuse therefore needs `vendor/libghostty-vt/`, `vendor/libghostty-vt.vendor.json`, `vendor/patches/` and both `*.patches.md` files.
- `crates/ghostty-vt`: 12,331 lines of Rust. `src/`: 291,081 lines of Rust.
- Intel macOS: the target is supported (`build.rs:12`). The release workflow produces `herdr-macos-x86_64` on an ARM runner (`release.yml:91-93`). **No native Intel build has been verified.**

## 2. Detection (`src/detect/`)

- `AgentState { Idle, Working, Blocked, Unknown }` (`mod.rs:11-20`). `AgentDetection` carries `visible_blocker` (`:33`).
- The `Agent` enum has 24 variants (`mod.rs:45-73`), 22 of which have screen manifests (`:100`).
- Entry point: `detect_agent_with_osc(Option<Agent>, &str, osc_title, osc_progress) -> AgentDetection` (`mod.rs:305`), which calls `manifest::detect_with_osc` (`manifest.rs:368`). `mod.rs` is 2,264 lines and `manifest.rs` is 1,536.
- **Coupling:**
  - `crate::platform::{ForegroundJob, Process}` for process identification (`mod.rs:249-381`)
  - `crate::config::config_dir` for local overrides (`manifest.rs:1121`)
  - `manifest_update.rs` uses `crate::events::AppEvent`, `crate::config::state_dir` and a `curl` subprocess
  - The matcher can be extracted with small stubs.
- **"Working" is not screen-only.** Working state is driven by PTY activity and stabilised in the pane layer: `src/pane/agent_detection.rs` (707 lines) and `terminal::state::stabilize_agent_detection`. An Idle → Working → Idle loop needs that layer ported too.
- **Manifests model interactive TUIs only.** Herdr does not identify `-p/--print` invocations (`mod.rs:821-850`). A known agent with no matching rule falls back to **Idle** (`manifest/tests.rs:92-104`). One-shot `claude --print` or `codex exec` output under a PTY will therefore read as Idle.
- **Rule schema** (`manifest.rs:153-177`, `deny_unknown_fields`): there is no keystroke or dismiss-action field. Auto-remediation keys would have to be authored and maintained per agent and per agent version.
- **Blocked rules that exist** (`*` = `visible_blocker`):
  - **claude:** `live_blocked_form*`, `live_blocked_form_named_session*`, `dynamic_workflow_prompt*`, `mcp_elicitation_prompt*`, `bash_permission_prompt*`, `generic_permission_prompt*`, `legacy_no_prompt_blocker`
  - **codex:** `osc_title_blocked*`, `trust_directory*`, `startup_update*`, `live_strong_blocker*`, `weak_blocker`
  - **antigravity (`agy`):** `permission_prompt*`, `question_prompt*`, `trust_prompt*`, `legacy_permission_prompt*`
  - **grok:** `osc_title_blocked*`, `option_dialog_blocked*`, `permission_hints_blocked*`, `question_dialog_hints_blocked*`, `permission_scope_selector*`
  - **gemini:** `apply_or_allow_change*`
  - **opencode:** `permission_required*`
  - **kimi:** `current_approval_panel*`, `question_panel*`, `legacy_approval_panel`
  - **qwen:** `osc_title_blocked*`, `waiting_for_confirmation*`, `tool_confirmation*`, `question_dialog*`, `folder_trust_dialog*`
  - **pi:** none
- **Rules that do not exist anywhere:** `startup_update` for claude and grok, any claude trust rule, `auth_error`, `rate_limit_modal`. There is no generic or cross-agent manifest.
- **Tests:** `manifest/tests.rs` has 24 tests against synthetic manifests. There is no corpus of captured screens; `scripts/capture_agent_screen.py` writes to the gitignored `.local/`. `AGENTS.md:201`: *"Do not add tests that classify captured or invented CLI screens against bundled agent rules, or freeze an agent's specific detection rule IDs."* Lope fixtures would have to be authored from scratch and kept in Lope's own repo.

## 3. PTY actor (`src/pty/`)

- `PtyIoActorHandle` lives in `src/pty/actor/unix.rs:94` (`actor.rs` is 842 lines, `fd.rs` 241, `backend.rs` 40).
- On Unix, `SubmitUserInput` is `{text, enter, delay, reply}`. There is **no `deadline`** (`unix.rs:76-81`); only the Windows variant has one (`actor.rs:48-54`). The reply confirms the write was queued and written, not that the key took effect.
- The Unix actor is Herdr's own fd/poll/thread loop, coupled to `crate::platform::PtyInputGuard`, `foreground_process_group_id_for_tty_fd`, `crate::thread_spawn` and `crate::HERDR_ENV_VAR`. portable-pty only opens the PTY and spawns the child (`backend/unix.rs:12-42`).
- Leader exit and PTY EOF are tracked separately: `child.wait()` in `spawn_blocking` emits `PaneDied` (`pane.rs:2747-2768`), and `on_reader_exit` handles EOF (`actor/unix.rs:581`).
- `setsid` is done by portable-pty (`vendor/portable-pty/src/unix.rs:257`).
- **Herdr does not `killpg`.** It enumerates the session with `getsid` (`platform/macos.rs:1126-1178`) and signals each pid HUP → TERM → KILL, with 250 ms grace between steps (`pane.rs:1760-1808`).

## 4. Server, API, remote, UI

- **Sockets:** `herdr.sock` (JSON API) and `herdr-client.sock` (binary client frames) (`session.rs:172,186`). `server/headless.rs` is 3,472 lines.
- **No peer-UID check.**
  - The peer PID (`LOCAL_PEERPID` on macOS, `SO_PEERCRED` on Linux) is used only in log descriptions (`unix_common.rs:402-415`).
  - Access control is the socket file mode 0600 (`api/server.rs:29`, `server/socket_paths.rs`).
  - There is no `getpeereid` or `LOCAL_PEERCRED` anywhere in `src/`.
- **API:** `EventHub` (`api/event_hub.rs`, 127 lines), `pane.wait_for_output` (`api/wait.rs:22`), `events.wait` (`api/server.rs:1530`), and `agent.prompt` with an optional `wait` and `agent.wait` (`api/schema/agents.rs:201-205`).
- **Remote:** `--machine <label|id>` (`cli/target.rs:221-235`) and `herdr machine add|list|…` (`cli/machine.rs:10-25`). The remote host must have Herdr installed and running; `machine add` installs it over scp (`remote/attach.rs:1098-1152`). The strings `traylinx-ci` and `hz` do not appear anywhere in Herdr.
- **UI:**
  - `src/ui/` is about 3.7k lines and depends on `crate::app::AppState`.
  - Real rendering is server-side compositing streamed over a binary protocol: `server/headless/render.rs` (1,011 lines), `render_stream.rs` (865), `protocol/wire.rs` (3,675), `client/shell` (about 22.9k), with cells from `pane/terminal.rs` (7,624).
  - A Lope "attach" view would be a new TUI built on `ghostty-vt`, not an extraction.

## 5. `AGENTS.md` principles

- **Confirmed:**
  - state is separated from runtime (`:30`)
  - platform code is isolated in `src/platform/<os>.rs` (`:33`)
  - detection is decoupled and reads a snapshot (`:34`)
  - "multiplicative performance paths" (`:38-43`)
- **Not confirmed:** Herdr's platform modules do **not** perform credential verification.

## 6. Licences

| Component | Licence | Source |
|---|---|---|
| Herdr | Apache-2.0 | `LICENSE` |
| `crates/ghostty-vt` (binding) | declares Apache-2.0 | `crates/ghostty-vt/Cargo.toml:6` |
| `vendor/libghostty-vt` | MIT | `vendor/libghostty-vt/LICENSE` |
| `vendor/portable-pty` | MIT (Wez Furlong) | `vendor/portable-pty/Cargo.toml` |

No relevant source file carries SPDX or licence headers. Reuse obligations are to ship the LICENSE files, a NOTICE crediting Herdr (Apache-2.0 §4), and to mark modified files. Copying code verbatim is **not** a clean-room implementation.

## 7. What Super Lope should take (feeds Spec 01 §6)

| Herdr asset | Decision | Why |
|---|---|---|
| `src/detect/manifest.rs` matcher + `manifests/*.toml` | Reuse in S9 (feature-gated), with stubs for `platform`/`config` | Self-contained matcher; data is valuable |
| `src/pane/agent_detection.rs` stabilisation | Port in S9 | Required for Idle → Working → Idle |
| `crates/ghostty-vt` + `vendor/libghostty-vt` | Reuse in S9 behind cargo feature `pty-pool` | Requires Zig 0.16 + network fetch; keep it out of the default build |
| `vendor/portable-pty` | Reuse in S9 | MIT, small |
| `src/pty/actor/*` | Re-implement (small tokio actor) | Heavy coupling to Herdr internals |
| `src/server/headless.rs`, `src/api/*` | Use as design reference only | Coupled to `AppState`; no peer-UID auth |
| `src/remote/*` | Not in v2.0 (L5) | Requires Herdr on the remote host |
| `src/ui/*`, client/render stack | Not in v2.0 (L6) | A rewrite, not an extraction |
