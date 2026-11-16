# Install Lope — any AI CLI, one prompt

You (the AI agent reading this) are going to install lope for your user. **Do not improvise. Do not skip steps. Do not invent flags.**

## Context

Lope is a multi-CLI validator ensemble for AI work. Any AI CLI drafts; any AI CLI validates. Current command surface as of v2.0.1 (`compat 0.17.3`):

- **Sprint commands**: `/lope-negotiate`, `/lope-execute`, `/lope-implement`, `/lope-audit`
- **Single-shot commands**: `/lope-ask`, `/lope-review`, `/lope-vote`, `/lope-compare`, `/lope-pipe`
- **Autonomous graph workflows**: `/lope-flow`
- **Memory and council review**: `/lope-memory`, `/lope-deliberate`
- **Evidence gates**: `lope gate`, `lope check`
- **Roster management**: `/lope-team`
- **Optional compression setup**: `/lope-headroom`
- **Meta and help**: `/lope-help`, `/lope`, `/using-lope`
- **Terminal maintenance**: `lope update` and `lope upgrade`

Gemini uses the `/lope:<verb>` namespaced form; Codex and Vibe load skills as content and invoke lope via natural language. Source: https://github.com/makakoo/lope (MIT, native Rust binary on macOS & Linux, with `0.17.3` Python compat rollback). Prebuilt release tarballs, source builds, and legacy Python wheel installs are supported.

## Prebuilt release & package installation

**Prebuilt Rust v2 release tarball (macOS/Linux, recommended):**

Download `lope-<target>.tar.gz` and `lope-<target>.tar.gz.sha256` (or `SHA256SUMS`) from [makakoo/lope releases](https://github.com/makakoo/lope/releases), verify checksum + Sigstore attestation, and run `./install.sh` (installs unprivileged into `$HOME/.local` by default):

```bash
shasum -a 256 -c lope-<target>.tar.gz.sha256   # Linux: sha256sum -c lope-<target>.tar.gz.sha256
tar -xzf lope-<target>.tar.gz
cd lope-<target>
./install.sh                                    # defaults to --prefix "$HOME/.local"
export PATH="$HOME/.local/bin:$PATH"            # add ~/.local/bin if not already in PATH
lope --version
lope configure
lope docs
```

If installing system-wide into `/usr/local`, run the privileged copy with `--skip-install` first, then run `lope install --host all` as your normal user so host skills never land in `/root/`:

```bash
sudo ./install.sh --prefix /usr/local --skip-install
lope install --host all
```

**Legacy Python compatibility package (`0.17.3`, fallback only):**

```bash
pipx install lope-agent
lope install --host codex  # choose your host, or all
lope configure
lope docs
```

Python 3.9+ and Bash 3.2+ are required for host installer hooks and the Python rollback path. Prebuilt GNU/Linux tarballs target `glibc >= 2.35` (`ubuntu-22.04` or newer); older distributions should build from source below. Do not layer two active `lope` binaries on `PATH` without checking `command -v lope`; if keeping `lope-py` for rollback, place `lope` (v2) ahead of `lope-py` on `PATH` or invoke fallback via `LOPE_FORCE_PYTHON=1 lope ...`.
Native Windows host installation is not qualified; use macOS or Linux.

Existing host files are moved to `${XDG_STATE_HOME:-~/.local/state}/lope/install-backups/`
before replacement. Backups preserve symlinks as links, not snapshots of their
external targets. Config, credentials, and runtime history are not rewritten.
Review and merge your custom skills from the printed backup path; restarting a CLI
is required to reload skills. Headroom remains optional and off by default.

The remaining steps describe installing from a `v2` Git checkout. Choose one route, not both.

## Prerequisites for building from Git checkout

- `git` on `PATH`
- Rust toolchain (`rust-toolchain.toml` pins `1.96.1`)
- `bash` 3.2+ (stock macOS works)
- `python3` >= 3.9 only if also running differential parity or Python rollback (`LOPE_FORCE_PYTHON=1`)

Verify with `git --version && rustc --version && bash --version | head -1`. If Rust is missing, install via `rustup`.

---

## Honest per-host support matrix

**Read this first.** Not every AI CLI supports user-installed slash commands. Pick your row before copying commands.

| Host | Slash commands? | Natural-language invocation? | Notes |
|---|---|---|---|
| **Claude Code** | ✅ full Lope skill set: `/lope`, `/lope-*`, `/using-lope` | ✅ | Scans `~/.claude/skills/` — symlinks just work |
| **Gemini CLI** | ✅ namespaced: `/lope:negotiate`, `/lope:execute`, `/lope:implement`, `/lope:audit`, `/lope:ask`, `/lope:review`, `/lope:vote`, `/lope:compare`, `/lope:pipe`, `/lope:team`, `/lope:flow`, `/lope:memory`, `/lope:deliberate`, `/lope:headroom`, `/lope:help` | ✅ | TOML files in `~/.gemini/commands/lope/` — note the colon, not hyphen |
| **OpenCode** | ✅ full Lope command set, including `/lope-flow`, `/lope-memory`, `/lope-deliberate`, `/lope-headroom` | ✅ | `~/.config/opencode/commands/*.md` (**plural**) with YAML frontmatter including `agent:` field |
| **Cursor** | ⚠️ unverified | ✅ | Skills written to `~/.cursor/agents/`. Test `/lope-*` after install; if they don't autocomplete, use natural language |
| **Mistral Vibe** | ❌ does **not** support user slash commands (confirmed by Vibe directly) | ✅ | Skills load as content at `~/.vibe/skills/`. Invoke via natural language |
| **Qwen Code** | ✅ via `~/.qwen/skills/` — same slash list as Claude | ✅ | Standard skill dir format |
| **pi (Traylinx)** | ✅ via shared `~/.agents/skills/` — same slash list | ✅ | Shared `@agents` skill tree; pi reads it at session start |
| **Codex** | ❌ does **not** register `/foo` from SKILL.md (confirmed by asking Codex directly) | ✅ | Skills load as **content**. Invoke via natural language: *"use lope to review auth.py across models"* and the Codex agent runs `lope <verb>` via bash |
| **GitHub Copilot CLI** | ❌ no user skill dir yet | ✅ | Invoke via natural language; agent runs `lope <verb>` via bash |

**Takeaway:** If your host is in the ❌ slash-commands column, lope still works perfectly. Users describe tasks in prose (*"plan the auth refactor"*, *"negotiate the Q4 campaign, needs to be right"*), the agent recognizes the shape and runs `lope negotiate "<goal>" --domain <engineering|business|research>` in a shell. Don't wait for an autocomplete that won't come.

---

## Step 1 — Clone, build, and install `makakoo/lope` (`v2`)

Clone `https://github.com/makakoo/lope.git` (or update an existing source checkout), build the native release binary, and install it to `~/.local/bin/lope` (or system-wide via `sudo`):

```bash
git clone -b v2 https://github.com/makakoo/lope.git /path/to/lope
cd /path/to/lope
./scripts/cargo build --release
mkdir -p "$HOME/.local/bin"
install -m 0755 target/release/lope "$HOME/.local/bin/lope"
# System-wide alternative: sudo install -m 0755 target/release/lope /usr/local/bin/lope
```

(If upgrading an existing machine that keeps `$HOME/.lope` for state and skills/rollback, avoid cloning a separate repo over `$HOME/.lope` if you already store `config.json` / `memory.db` there, or keep the source checkout elsewhere and install `lope` + host skills via `lope install`.)

## Step 2 — Run the bundled installer

From the repository checkout (or pass `LOPE_SOURCE_DIR=/path/to/lope` when running from another directory), ensure your install bin dir is on `PATH` and register skills and commands for all detected hosts:

```bash
LOPE_SOURCE_DIR="$PWD" "$HOME/.local/bin/lope" install --host all
```

(For packaged tarball installs built with `./scripts/package-release.sh`, bundled assets live under `<prefix>/share/lope`, so `lope install --host all` works from any working directory without `LOPE_SOURCE_DIR`.)

The installer covers all 8 hosts (claude, codex, gemini, opencode, cursor, vibe, qwen, pi). It writes the standard locations for Claude, Codex, Gemini, OpenCode, and Cursor, and skips Vibe/Qwen/pi when their host homes are absent. Prints which slash commands should be available in which host.

### Alternative: per-host install (if the bundled installer fails)

Identify which CLI you're running inside and execute the matching block below. Skip the others.

#### Claude Code

```bash
mkdir -p "$HOME/.claude/skills"
for skill in lope lope-negotiate lope-execute lope-implement lope-audit lope-ask lope-review lope-vote lope-compare lope-pipe lope-team lope-flow lope-memory lope-deliberate lope-headroom lope-help using-lope; do
  target="$HOME/.claude/skills/$skill"
  [ -e "$target" ] || [ -L "$target" ] && rm -rf "$target"
  ln -s "$HOME/.lope/skills/$skill" "$target"
done
```

Available after restart: `/lope`, `/using-lope`, and the full `/lope-*` skill set (`negotiate`, `execute`, `implement`, `audit`, `ask`, `review`, `vote`, `compare`, `pipe`, `team`, `flow`, `memory`, `deliberate`, `headroom`, `help`).

#### Gemini CLI

```bash
mkdir -p "$HOME/.gemini/commands/lope"
for toml in negotiate.toml execute.toml implement.toml audit.toml ask.toml review.toml vote.toml compare.toml pipe.toml team.toml flow.toml memory.toml deliberate.toml headroom.toml help.toml; do
  target="$HOME/.gemini/commands/lope/$toml"
  [ -e "$target" ] || [ -L "$target" ] && rm -f "$target"
  ln -s "$HOME/.lope/commands/lope/$toml" "$target"
done
```

Available after restart: `/lope:negotiate`, `/lope:execute`, `/lope:implement`, `/lope:audit`, `/lope:ask`, `/lope:review`, `/lope:vote`, `/lope:compare`, `/lope:pipe`, `/lope:team`, `/lope:flow`, `/lope:memory`, `/lope:deliberate`, `/lope:headroom`, `/lope:help`. Gemini uses **colon** not hyphen.

#### OpenCode

```bash
mkdir -p "$HOME/.config/opencode/commands"   # plural "commands"
for md in lope.md lope-negotiate.md lope-execute.md lope-implement.md lope-audit.md lope-ask.md lope-review.md lope-vote.md lope-compare.md lope-pipe.md lope-team.md lope-flow.md lope-memory.md lope-deliberate.md lope-headroom.md lope-help.md using-lope.md; do
  target="$HOME/.config/opencode/commands/$md"
  [ -e "$target" ] || [ -L "$target" ] && rm -f "$target"
  ln -s "$HOME/.lope/commands/opencode/$md" "$target"
done
```

Available after restart: the full OpenCode command set — `/lope`, `/using-lope`, and `/lope-*` for negotiate, execute, implement, audit, ask, review, vote, compare, pipe, team, flow, memory, deliberate, headroom, and help. The `.md` files in `commands/opencode/` already have the required YAML frontmatter (`name`, `description`, `agent: build`) — do not replace them with raw SKILL.md symlinks, OpenCode will reject them.

#### Codex — content install only, no slash commands

Codex does not surface user `SKILL.md` files as slash commands. Install them anyway so the Codex agent can read them as context and invoke lope via natural language:

```bash
mkdir -p "$HOME/.codex/skills"
for skill in lope lope-negotiate lope-execute lope-implement lope-audit lope-ask lope-review lope-vote lope-compare lope-pipe lope-team lope-flow lope-memory lope-deliberate lope-headroom lope-help using-lope; do
  target="$HOME/.codex/skills/$skill"
  [ -e "$target" ] || [ -L "$target" ] && rm -rf "$target"
  mkdir -p "$target"
  cp -R "$HOME/.lope/skills/$skill"/. "$target"/
done
```

Tell the user: *"Typing `/lope` in Codex does nothing — Codex doesn't support user slash commands. Instead, describe the task in prose (e.g. 'plan the auth refactor, needs to be right') and Codex will invoke `lope` via bash."*

#### Mistral Vibe — content install only, no slash commands

Same story as Codex. Skills install as context, not slash commands.

```bash
mkdir -p "$HOME/.vibe/skills"
for skill in lope lope-negotiate lope-execute lope-implement lope-audit lope-ask lope-review lope-vote lope-compare lope-pipe lope-team lope-flow lope-memory lope-deliberate lope-headroom lope-help using-lope; do
  target="$HOME/.vibe/skills/$skill"
  [ -e "$target" ] || [ -L "$target" ] && rm -rf "$target"
  ln -s "$HOME/.lope/skills/$skill" "$target"
done
```

#### Cursor — unverified

```bash
mkdir -p "$HOME/.cursor/agents"
for skill in lope lope-negotiate lope-execute lope-implement lope-audit lope-ask lope-review lope-vote lope-compare lope-pipe lope-team lope-flow lope-memory lope-deliberate lope-headroom lope-help using-lope; do
  src="$HOME/.lope/skills/$skill/SKILL.md"
  target="$HOME/.cursor/agents/$skill.md"
  [ -f "$src" ] || continue
  [ -e "$target" ] || [ -L "$target" ] && rm -f "$target"
  ln -s "$src" "$target"
done
```

After Cursor restart, check if `/lope-*` autocompletes. If yes, great. If no, invoke lope via natural language — the skills still load as context.

#### Qwen Code

```bash
mkdir -p "$HOME/.qwen/skills"
for skill in lope lope-negotiate lope-execute lope-implement lope-audit lope-ask lope-review lope-vote lope-compare lope-pipe lope-team lope-flow lope-memory lope-deliberate lope-headroom lope-help using-lope; do
  target="$HOME/.qwen/skills/$skill"
  [ -e "$target" ] || [ -L "$target" ] && rm -rf "$target"
  ln -s "$HOME/.lope/skills/$skill" "$target"
done
```

Available after restart: `/lope`, `/using-lope`, and the full `/lope-*` skill set.

#### pi (Traylinx) — shared `@agents` skill tree

pi reads skills from the cross-CLI `~/.agents/skills/` directory (shared with any other `@agents` consumer).

```bash
mkdir -p "$HOME/.agents/skills"
for skill in lope lope-negotiate lope-execute lope-implement lope-audit lope-ask lope-review lope-vote lope-compare lope-pipe lope-team lope-flow lope-memory lope-deliberate lope-headroom lope-help using-lope; do
  target="$HOME/.agents/skills/$skill"
  [ -e "$target" ] || [ -L "$target" ] && rm -rf "$target"
  ln -s "$HOME/.lope/skills/$skill" "$target"
done
```

Available after pi restart: `/lope`, `/using-lope`, and the full `/lope-*` skill set. Note: the install also registers pi as a lope **validator** (via `pi -p "{prompt}"`), so pi can both invoke lope AND be invoked BY lope for cross-model review.

#### Any other AI CLI (generic)

You know where your own skills or commands live. The Lope skills are in `~/.lope/skills/`:

```
$HOME/.lope/skills/lope/SKILL.md
$HOME/.lope/skills/lope-negotiate/SKILL.md
$HOME/.lope/skills/lope-execute/SKILL.md
$HOME/.lope/skills/lope-implement/SKILL.md
$HOME/.lope/skills/lope-audit/SKILL.md
$HOME/.lope/skills/lope-ask/SKILL.md
$HOME/.lope/skills/lope-review/SKILL.md
$HOME/.lope/skills/lope-vote/SKILL.md
$HOME/.lope/skills/lope-compare/SKILL.md
$HOME/.lope/skills/lope-pipe/SKILL.md
$HOME/.lope/skills/lope-team/SKILL.md
$HOME/.lope/skills/lope-flow/SKILL.md
$HOME/.lope/skills/lope-memory/SKILL.md
$HOME/.lope/skills/lope-deliberate/SKILL.md
$HOME/.lope/skills/lope-headroom/SKILL.md
$HOME/.lope/skills/lope-help/SKILL.md
$HOME/.lope/skills/using-lope/SKILL.md
```

Symlink them into your host's native skill path, or copy if symlinks aren't supported, or translate the frontmatter to whatever format your host requires. If you genuinely can't figure out where, file an issue at https://github.com/makakoo/lope/issues with the host name.

---

## Step 3 — Verify the binary & config

```bash
lope --version
lope status
```

`--version` prints `lope 2.0.1 (compat 0.17.3)`. `status` lists detected AI CLIs. Either failing means the binary is not on `PATH` or config is unreadable — report the error.

To refresh host skills or update the release tarball, re-run `./install.sh --prefix /usr/local` or:

```bash
lope install --host all
```

`lope update` / `lope upgrade` delegate to the Python `0.17.3` updater in v2.0.1 for git-backed compatibility installs; for prebuilt native v2 tarballs, download the latest release from `https://github.com/makakoo/lope/releases` and re-run `./install.sh`, or rebuild from the `v2` branch.

## Step 4 — Ensure install bin directory is on `PATH`

If you installed to a custom prefix (for example `$HOME/.local`), make sure `$HOME/.local/bin` is ahead of any legacy wrapper on `PATH` (for example in `~/.zshrc` or `~/.bashrc`):

```bash
export PATH="$HOME/.local/bin:$PATH"
```

**Do not edit the user's shell rc yourself without asking.**

## Step 5 — Tell the user to restart their CLI

Slash command discovery is cached at CLI **session start**. The commands you just installed will **not** appear in the current session. Tell the user explicitly:

> Lope is installed. **Quit this CLI completely and reopen it** — `/lope-*` commands load at startup. For Codex and Vibe there are no slash commands by design; describe your task in prose and the agent will run `lope` for you.

## Step 6 — Smoke test

After the user restarts, they can verify install worked by running either of these in any CLI:

1. **Slash command path** (Claude Code, Gemini, OpenCode, Cursor): type `/lope-help` and confirm the agent dumps the complete lope reference.
2. **Natural-language path** (Codex, Vibe, or any CLI): type *"show me the lope docs"* — the agent should run `lope docs` in bash and display the reference.

Either path should end with the user seeing the structured lope reference.

---

## What lope needs to actually work

Lope needs **at least two different AI CLIs installed on the machine** to form a real ensemble for `negotiate/execute/audit`. The single-shot verbs (`ask/review/vote/compare/pipe`) run with one validator but produce less useful output — the whole point is multi-model perspective. Auto-detects 15: Claude Code, OpenCode, Gemini CLI, Codex, Mistral Vibe, Aider, Ollama, Goose, Open Interpreter, llama.cpp, GitHub Copilot CLI, Amazon Q, pi (Traylinx), Qwen Code, and Agy. If the user has only one, tell them to install one more before running lope.

## Hard rules — do not break these

- **Do not invent flags.** Each verb has its own fixed flag surface; run `lope <verb> --help` or `lope docs` if unsure. Key flags: `negotiate` takes `--domain/--out/--max-rounds/--context`; `ask/review/vote/compare/pipe` all take `--json`; `vote` requires `--options`; `compare` accepts `--criteria`; `review` accepts `--focus`; `pipe` accepts `--require-all`.
- **Do not write a wrapper script around lope.** Lope is already a CLI.
- **Do not commit lope state to the user's project git repo** unless they explicitly ask.

## Uninstall

```bash
rm -rf "$HOME/.lope" \
       "$HOME/.claude/skills/lope" "$HOME/.claude/skills/lope-"* "$HOME/.claude/skills/using-lope" \
       "$HOME/.codex/skills/lope" "$HOME/.codex/skills/lope-"* "$HOME/.codex/skills/using-lope" \
       "$HOME/.vibe/skills/lope" "$HOME/.vibe/skills/lope-"* "$HOME/.vibe/skills/using-lope" \
       "$HOME/.qwen/skills/lope" "$HOME/.qwen/skills/lope-"* "$HOME/.qwen/skills/using-lope" \
       "$HOME/.agents/skills/lope" "$HOME/.agents/skills/lope-"* "$HOME/.agents/skills/using-lope" \
       "$HOME/.gemini/commands/lope" \
       "$HOME/.config/opencode/commands/lope"*.md "$HOME/.config/opencode/commands/using-lope.md" \
       "$HOME/.config/opencode/command/lope"*.md "$HOME/.config/opencode/command/using-lope.md" \
       "$HOME/.cursor/agents/lope"*.md "$HOME/.cursor/agents/using-lope.md"
```

## Troubleshooting

- **`/lope*` doesn't autocomplete** after install and restart in a ✅-slash-command host → check `ls ~/.claude/skills/ | grep lope` (or equivalent path). If the symlinks are missing, re-run `~/.lope/install`.
- **`/lope*` doesn't autocomplete in Codex/Vibe** → that's by design. Those hosts don't surface user slash commands. Use natural language instead.
- **`/lope*` doesn't autocomplete in OpenCode** → confirm the directory is `commands/` (plural) not `command/` (singular). v0.3.1 shipped with the wrong path; v0.3.2 cleans up the old dir automatically.
- **`lope status` shows 0 detected CLIs** → no AI CLIs on `$PATH`; install at least 2.
- **`lope negotiate` crashes** → capture the full traceback and open an issue. Do not patch lope source.

## You are done when

1. `lope version` prints the new version banner
2. `lope status` lists at least one detected CLI
3. You told the user explicitly to **restart their CLI** (for slash-command hosts) or to **invoke lope via natural language** (for content-only hosts)
4. The user has successfully seen the lope reference via either `/lope-help` or prose ("show me the lope docs")

Final message:

> Lope is installed. **Slash-command hosts** (Claude Code, Gemini CLI, OpenCode): quit and reopen your CLI, then try `/lope-help` for the full reference or `/lope-negotiate "your first goal"` to draft a sprint. **Content-only hosts** (Codex, Vibe): describe your task in prose — I'll invoke lope for you. Run `lope docs` in a terminal anytime for the complete reference.
