# S9 — PTY warm pool and daemon (gated)

[Sprints](README.md) · Previous: [S8](S8-cutover.md)

## Gate

S9 starts only if OQ-4 approves it. The approval must be based on ≥ 7 days of FR12 diagnostics (collected since S4) showing that a material share of `provider_timeout`s are interactive blockers, or that cold-start latency dominates multi-round workflows. If the diagnostics show slow models or network stalls instead, S9 is cancelled and recorded as such.

## Goal

An opt-in Tier 3: warm interactive agent panes in `loped` for providers that request it, with Lope-authored blocker handling. This is feature `pty-pool`, off in default builds.

## Tasks

1. **Build prerequisites** (DEC-5):
   - Zig 0.16;
   - vendor `libghostty-vt` with its provenance files and patches, `crates/ghostty-vt`, and `vendor/portable-pty` with a `[patch.crates-io]` entry;
   - NOTICE and LICENSE files (NFR12);
   - an offline path via `LIBGHOSTTY_VT_ZIG_SYSTEM_DIR`;
   - a native x86_64 build.
2. **`lope-pty`:**
   - a new tokio PTY actor on portable-pty;
   - the Herdr detection matcher and manifests, with stubs for `platform` and `config`;
   - a port of the pane-layer stabiliser (`src/pane/agent_detection.rs`).
3. **`lope-daemon` / `loped`** (FR40):
   - socket under `~/.lope/v2/run/` (needs PL-2);
   - `getpeereid` UID check;
   - protocol defined in one place (Spec 04 §7);
   - launchd `com.makakoo.lope` with the NFR16 environment, or a Makakoo service plugin (PM-8).
4. **Warm pool** (FR41):
   - readiness from detection plus the stabiliser;
   - prompt delivery by bracketed paste or file reference;
   - responses extracted from the raw stream;
   - session reset per agent;
   - silent fallback to T2 through `lope-pool-client`.
5. **Blocker policy** (FR42):
   - rules only for existing rule ids;
   - per-agent, per-version dismiss keys in Lope's rule file;
   - read-only verbs never approve tools (L9);
   - Lope-owned fixtures.
6. **`lope daemon|pool|detect`** (FR43).

## Acceptance

- A18 passes; idle `loped` uses < 15 MB (NFR2).
- With the pool enabled for one provider, M1 shows no regression vs T2.
- Evidence: `docs/evidence/s9-pool.txt`.
