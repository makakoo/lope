# Audit 03: Makakoo OS & Glimmer Compatibility Seams

[Audits Index](README.md) · Previous: [02 — Rust Herdr Audit](02-herdr-rust-audit.md) · Next: [Specs Index](../specs/README.md)

Verified against `/Users/sebastian/projects/apps/makakoo-os`, `/Users/sebastian/MAKAKOO`, and `/Users/sebastian/Projects/apps/makakoo-jungle` on `2026-11-07`.

---

## 1. Glimmer (`makakoo-jungle`) Invocation Seams

Glimmer (`com.makakoo.glimmer`, CLI `/usr/local/bin/glimmer`, companion daemon `com.makakoo.jungle`) invokes `/usr/local/bin/lope` directly across multiple seams defined in `/Users/sebastian/Projects/apps/makakoo-jungle/docs/specs/04-interfaces-and-protocol.md`.

**Super Lope must satisfy every single seam below as a 100% drop-in replacement:**

| Glimmer Seam | Exact Command / Path Invoked by Glimmer | Required Super Lope Behavior |
|---|---|---|
| **Doctor Check (`glimmer doctor` & `A1`)** | `lope version` (under `env -i HOME="$HOME" PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin:/opt/homebrew/bin"`) | Exit `0` and print version string containing a semver token `>= 0.17.3` (e.g. `lope 2.0.0 (super-lope; compat 0.17.3)`). Also accept `lope --version` and `lope -V`. |
| **Status & Lease Inspection** | `lope status` | Print primary validator, enabled validator list, parallel setting, timeout, and manifest summary (`N manifest(s): active=X, abandoned=Y`) read from `~/.lope/state/sprints/`. |
| **Stale Lease Pruning (`S0` / `S8` / `FR34`)** | `lope jobs prune --stale` | Inspect `~/.lope/state/sprints/`, verify recorded PIDs are dead (`kill(pid, 0)` + start fingerprint check), archive/prune stale manifests safely without touching live runs, and exit `0`. |
| **Watched Council Review (`S3` / `S5` / `FR8`–`FR10`)** | `lope review <single-file> --consensus [quorum] --format markdown [--output <path>] [--focus <lens>] [--brain-context <query>] [--remember] [--brain-log]` | Accept **one** positional `<file>`, support both `--consensus` (boolean flag defaulting to `quorum`) and `--consensus <mode>`, stream progress to terminal/stdout, write clean UTF-8 Markdown to `--output <path>` when provided (and stdout when omitted), and exit `0` on completion. |
| **7 Role Lenses (`FR38`)** | `--focus <lens>` passed to `lope review` | Preserve custom focus strings and built-in role lenses (`product`, `sales`, `tax`, `finance`, `legal`, `research`, `engineering` or full prose descriptions). |
| **Plan Sprint (`S7` / `FR16` / `FR38`)** | `lope negotiate <goal> [--domain <engineering\|business\|research>] [--out <path>] [--brain-context <query>]` | Draft `SPRINT-<SLUG>.md` in the current working directory (or `--out`), configuring domain-specific sections (`Files`/`Tests` for `engineering`, `Deliverables`/`Success Metrics` for `business`, `Artifacts`/`Validation Criteria` for `research`). |
| **Child-Worktree Build (`S7` / `FR17`–`FR18`)** | `lope implement <absolute-sprint-doc-path>` (with `cwd` set to the child git worktree) | Execute phases autonomously in `cwd` using the configured implementer and validator ensemble, respecting `LOPE_IMPLEMENTATION_DEPTH`. |
| **Fake-Lope Test Seam (`S3`)** | `scripts/fake-lope` prepended to `PATH` in `S3` test | Glimmer resolves `lope` via `PATH`, so installing Super Lope at `/usr/local/bin/lope` preserves `scripts/fake-lope` PATH-override testing. |

---

## 2. Makakoo OS (`0.4.0`) Integration Seams

Verified against `/Users/sebastian/projects/apps/makakoo-os/Cargo.toml` and `/Users/sebastian/.lope/lope/makakoo_bridge.py`:

1. **Environment Variables**:
   - `MAKAKOO_HOME` (resolves to `/Users/sebastian/MAKAKOO`, fallback `$HARVEY_HOME` or `~/MAKAKOO`).
   - `MAKAKOO_BIN` (optional override for `makakoo` binary path).
   - `LOPE_BRAIN_AUTOMEMORY` (`1`/`true` enables curated auto-memory writes to `$MAKAKOO_HOME/data/auto-memory/`).
   - `LOPE_MEMORY` (`off` disables `~/.lope/memory.db`).
2. **Brain Search (`--brain-context <query>`)**:
   - Invokes `makakoo search <query>` (or reads the Brain FTS5 SQLite index directly in read-only mode when local), scrubs secrets via redaction rules, trims to `--brain-budget` (default `1,200` tokens / `~4,800` chars), and wraps the result in the canonical advisory block so it is prepended once to validator prompts.
3. **Brain Journal Logging (`--brain-log`)**:
   - Appends a single Logseq outliner bullet (`- [[Lope]] ...`) to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md` and immediately executes `makakoo sync` so the new entry is indexed in Brain FTS5 (`FR11`).
4. **Headroom Context Compression**:
   - Respects `headroom_compress` / `headroom_retrieve` boundaries when installed: never compresses secrets, exact diffs, or `---VERDICT---` blocks.
5. **Rust Toolchain Alignment (`makakoo-os/Cargo.toml`)**:
   - `makakoo-os` uses Rust `2021`, `tokio = "1.40"`, `rusqlite = { version = "0.32", features = ["bundled-full"] }`, `reqwest = { version = "0.12", features = ["json", "rustls-tls", "stream"] }`, `clap = "4"`, `serde = "1"`, `serde_json = "1"`, `blake3 = "1.5"`, `sha2 = "0.10"`, `regex = "1.11"`.
   - Sharing these exact crate versions in Super Lope guarantees zero TLS/SQLite native C-library conflicts (`bundled-full` SQLite + pure-Rust `rustls-tls`).

---

## 3. Installed Host Skills Seam (`~/.gemini/skills/lope*`)

Twelve skill packages are installed across the machine (`/Users/sebastian/.gemini/skills/`):
- `using-lope`, `lope`, `lope-ask`, `lope-audit`, `lope-compare`, `lope-deliberate`, `lope-execute`, `lope-flow`, `lope-headroom`, `lope-help`, `lope-implement`, `lope-memory`, `lope-negotiate`, `lope-pipe`, `lope-review`, `lope-team`, `lope-vote`.

Every `SKILL.md` invokes `lope <subcommand> [flags]`. Because Super Lope preserves the exact CLI grammar while adding new subcommands (`lope daemon`, `lope pool`, `lope attach`, `lope detect`), all 17 skill packages continue to work out-of-the-box with zero edits required, and `lope install` updates host skills with documentation for the new warm-pool and live-TUI capabilities.

---

[Audits Index](README.md) · Previous: [02 — Rust Herdr Audit](02-herdr-rust-audit.md) · Next: [Specs Index](../specs/README.md)
