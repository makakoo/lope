# Audit 02: Rust Herdr (`0.9.5`)

[Audits Index](README.md) · Previous: [01 — Python Lope Audit](01-lope-python-audit.md) · Next: [03 — Makakoo & Glimmer Seams](03-makakoo-and-glimmer-seams-audit.md)

Verified against `/Users/sebastian/projects/apps/herdr` on `2026-11-07` (`HEAD` at `ee5bfb50`, version `0.9.5`, license `Apache-2.0`).

---

## 1. Repository & Crate Architecture

- **Location**: `/Users/sebastian/projects/apps/herdr`
- **Language & Toolchain**: Rust `2021` edition (`Cargo.toml`), workspace members `[".", "crates/ghostty-vt"]`, patched `portable-pty = "=0.9.0"` (`vendor/portable-pty`).
- **Total Source Volume**: **`291,081` lines of Rust** across `src/`, plus `crates/ghostty-vt` (Ghostty terminal VT parser/screen buffer crate) and `2,186` lines of declarative TOML detection rules across `22` files in `src/detect/manifests/`.
- **Core Dependencies (`Cargo.toml`)**:
  - Async & IPC: `tokio = "1"` (`rt-multi-thread`, `macros`, `sync`, `time`, `process`, `io-util`, `signal`), `interprocess = "2.4.2"`, `bincode = "2"`
  - Terminal & TUI: `ghostty-vt`, `portable-pty = "0.9.0"`, `ratatui = "0.30"`, `crossterm = "0.29"`
  - Serialization & Config: `serde = "1"`, `serde_json = "1"`, `toml = "0.8"`, `toml_edit = "0.22"`, `jsonc-parser = "0.33.1"`, `schemars = "1.2.1"`
  - Utilities: `clap = "4.5"`, `regex = "1"`, `sha2 = "0.10"`, `tracing = "0.1.44"`

---

## 2. Subsystem Audit: What Super Lope Extracts & Reuses

### 2.1 Virtual Terminal & PTY Actor (`crates/ghostty-vt` & `src/pty/`)
- **`crates/ghostty-vt`**: High-speed Virtual Terminal state machine that parses arbitrary ANSI/VT sequences, OSC titles (`osc_title`), alternate screen buffers, and scrollback into an inspectable grid of lines and cells without needing a visible window.
- **`src/pty/actor.rs` (`842` LOC), `src/pty/fd.rs` (`241` LOC), `src/pty/backend.rs` (`40` LOC)**:
  - `PtyIoActorHandle` decouples non-blocking PTY reads/writes from state management using bounded `tokio::sync::mpsc` channels.
  - Supports structured `SubmitUserInput { text, enter, delay, deadline, reply }` so prompts and keystrokes (`Enter`, `Esc`, arrow keys) can be reliably injected into interactive CLI sessions with confirmation that the Enter keystroke landed.

### 2.2 Evidence-Based Agent Detection Engine (`src/detect/`)
- **`src/detect/mod.rs` (`2,264` LOC) & `src/detect/manifest.rs` (`1,465` LOC)**:
  - Evaluates screen snapshots (`bottom_non_empty_lines(N)`, `top_non_empty_lines(N)`, `osc_title`, `after_last_prompt_marker`, `after_last_horizontal_rule`, `last_non_empty_above_prompt_box`) against prioritized AND/OR regex and substring rules.
  - Emits `AgentDetection`:
    ```rust
    pub enum AgentState {
        Idle,     // Prompt visible, ready for next instruction
        Working,  // Spinner / timer / tool call active
        Blocked,  // Approval, trust dialog, question, or update prompt waiting on user
        Unknown,  // Unclassified shell or viewer
    }
    ```
- **22 Built-In Agent Manifests (`src/detect/manifests/*.toml`)**:
  - `amp.toml` (`65` LOC), `antigravity.toml` (`80` LOC, `id = "agy"`), `claude.toml` (`242` LOC), `cline.toml` (`63` LOC), `codex.toml` (`129` LOC), `cursor.toml` (`57` LOC), `devin.toml` (`86` LOC), `droid.toml` (`48` LOC), `gemini.toml` (`25` LOC), `github-copilot.toml` (`45` LOC), `grok.toml` (`161` LOC), `hermes.toml` (`102` LOC), `kilo.toml` (`24` LOC), `kimi.toml` (`77` LOC), `kiro.toml` (`97` LOC), `letta.toml` (`95` LOC), `maki.toml` (`68` LOC), `muse.toml` (`113` LOC), `opencode.toml` (`37` LOC), `pi.toml` (`27` LOC), `qodercli.toml` (`38` LOC), `qwen.toml` (`119` LOC).
  - Every manifest encodes explicit `state = "blocked"` rules with `visible_blocker = true` (e.g., `codex.toml` `trust_directory`, `startup_update`, `live_strong_blocker`; `claude.toml` `live_blocked_form`, `mcp_elicitation_prompt`; `antigravity.toml` `permission_prompt`, `question_prompt`, `trust_prompt`).

### 2.3 Headless Server & Socket API (`src/server/headless.rs` & `src/api/`)
- **`src/server/headless.rs` (`3,472` LOC)**:
  - Runs the full event loop without entering raw terminal mode or reading `stdin`.
  - Listens on Unix domain sockets (`herdr.sock` for JSON RPC and `herdr-client.sock` for binary TUI frames).
  - Maintains `AppState` and all live PTYs across client disconnects and `Cmd-Q` window closes.
- **`src/api/server.rs` (`1,850` LOC), `src/api/subscriptions.rs` (`1,014` LOC), `src/api/wait.rs` (`854` LOC)**:
  - Implements event-driven waiting (`events.wait`, `pane.wait_for_output`, `agent.prompt --wait`) with zero busy-polling from external callers.
  - Tracks state transitions (`Working -> Idle`, `Working -> Blocked`) and fires immediate event notifications via `EventHub` (`src/api/event_hub.rs`).

### 2.4 Remote SSH Multi-Machine Orchestration (`src/remote/`)
- **`src/remote/attach.rs` (`6,241` LOC), `src/remote/process.rs` (`468` LOC), `src/remote/saved.rs` (`249` LOC)**:
  - Connects to saved remote SSH hosts (`traylinx-ci` / `hz`, GPU pods, cloud boxes) and exposes remote agent panes through the exact same API (`--machine <label>`).

---

## 3. Architectural Principles Inherited from Herdr (`AGENTS.md`)

Super Lope adopts Herdr's core engineering invariants verbatim:
1. **State is separated from runtime**: Pure state structs (`EnsembleState`, `DeliberationState`, `FlowState`, `PaneState`) are testable deterministically without spawning PTYs or network sockets.
2. **Detection is decoupled**: The detector reads an immutable screen snapshot (`ghostty-vt` grid) and never mutates parser or viewport state.
3. **Multiplicative path discipline**: No filesystem I/O, process-tree `ps` spawning, or full-scrollback allocations inside per-byte PTY read loops.
4. **Platform code isolation**: OS-specific process group and credential verification (`LOCAL_PEERCRED` / `getpeereid` on macOS/Linux, Job Objects on Windows) live in `platform/<os>.rs`.

---

[Audits Index](README.md) · Previous: [01 — Python Lope Audit](01-lope-python-audit.md) · Next: [03 — Makakoo & Glimmer Seams](03-makakoo-and-glimmer-seams-audit.md)
