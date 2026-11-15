# Releasing Lope

This is the release playbook. Follow it in order, don't improvise, don't skip steps. If a step is unclear, stop and fix the doc before shipping.

## Versioning — SemVer with lope-specific rules

Lope follows [Semantic Versioning](https://semver.org/) with these concrete rules:

| Bump | When |
|---|---|
| **MAJOR** (`0.3.x → 1.0.0`, `1.x → 2.0.0`) | Breaking changes to `~/.lope/config.json` schema, validator adapter interface, sprint doc format, or slash command names. Anything that makes a user's existing setup stop working. |
| **MINOR** (`0.3.x → 0.4.0`) | New features: new validator adapter, new host support, new CLI subcommand, new domain, new env flag, new architectural move (evidence gate, two-stage review, etc.). Backwards-compatible. |
| **PATCH** (`0.3.0 → 0.3.1`) | Install fixes, docs corrections, typo fixes, bugfixes that don't add features, INSTALL.md updates, bash installer tweaks. Must not introduce new public behavior. |

Pre-1.0 (`0.x.y`) means the config schema and adapter interface can still change without a MAJOR bump. The MAJOR column above takes effect at `1.0.0`. Until then, breaking changes bump MINOR and get called out loudly in the CHANGELOG.

## Version groups — Product v2 vs Python compat

Lope v2 tracks two intentionally distinct version groups checked by `./scripts/check-version.sh` and `tests/test_release_criteria.py`:

1. **Product v2 (`2.0.0`)** — native Rust CLI + installer + host extensions:
   - `Cargo.toml` — `[workspace.package] version = "X.Y.Z"` (propagated to all crates + `lope_core::PRODUCT_VERSION`)
   - `install` — `echo "Lope vX.Y.Z — ..."` banner
   - `.claude-plugin/plugin.json` — `"version": "X.Y.Z"`
   - `.cursor-plugin/plugin.json` — `"version": "X.Y.Z"`
   - `gemini-extension.json` — `"version": "X.Y.Z"`
2. **Python compatibility distribution (`0.17.3`)** — frozen v0.x rollback & parity baseline:
   - `pyproject.toml` — `[project] version = "0.17.3"` (`lope-agent` wheel for rollback/parity tests)
   - `lope/__init__.py` — `__version__ = "0.17.3"`
   - `crates/lope-core/src/lib.rs` — `COMPAT_VERSION = "0.17.3"`
   - `crates/lope-compat/grammar/argparse-python-lope.json` — `"lope_version": "0.17.3"`

Use `./scripts/bump-version.sh NEW_VERSION` for product releases — it updates all 5 product surfaces and runs `./scripts/check-version.sh`. Never edit them by hand. Keep the Python compat group untouched unless intentionally updating the compat baseline and regenerating parity snapshots.

## Preparation is not publication

A local candidate, version bump, successful tests and installed-file sync do not
publish a release. Commit, integration/merge, push, tag publication and hosted
release actions require explicit approval. `.github/workflows/publish.yml` builds
and verifies POSIX release tarballs (`aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`), generates `SHA256SUMS` +
Sigstore build provenance (`actions/attest-build-provenance`), and publishes GitHub
Releases on strict SemVer `vX.Y.Z[-prerelease]` tags matching `Cargo.toml` (`lope-cli`).
Python `lope-agent` compatibility wheels remain frozen at `0.17.3` and are not
republished by `v2.*` release tags.

Inspect source and installed checkouts separately. Preserve unrelated dirty work;
do not use a reset, stash, broad reinstall or tag deletion as a synchronization
shortcut. If the source branch is behind main, prepare the integration plan and
obtain approval before changing branch ancestry. Never silently discard installed
adapter fixes or locally modified skills.

## Release checklist

Copy this into the PR description (or into the commit body for a direct-to-main release). Check each box in order.

### Pre-flight

- [ ] **Reviewed working tree.** Account for every release change; preserve unrelated dirty/untracked operator state without staging it. No accidental stash.
- [ ] **On `main`.** `git rev-parse --abbrev-ref HEAD` prints `main`. Never tag a release from a feature branch.
- [ ] **Up to date.** `git pull --ff-only origin main` reports no changes pulled.
- [ ] **CHANGELOG draft.** Write the new section at the top of `CHANGELOG.md` under `# Changelog`. Format: `## X.Y.Z — <one-line tagline>` followed by bullet points of what changed, grouped by the reader's interest (user-facing features first, install/docs fixes second, internals last).
- [ ] **Nothing secret in the diff.** Run the full configured secret scan and review the staged patch. Credential-shaped fixtures, environment-variable names and local paths may match a grep; examine each match rather than treating grep alone as proof. Never publish operator config, private review transcripts or local runtime stores.

### Version bump

- [ ] **Pick the bump level.** MAJOR / MINOR / PATCH per the table above. Write the reason in one sentence before you touch files.
- [ ] **Bump product v2 locations.** `./scripts/bump-version.sh NEW_VERSION`
- [ ] **Verify sync.** `./scripts/check-version.sh` exits 0 and prints the new product version plus the `0.17.3` compat pin.
- [ ] **Binary reports new version.** `./scripts/cargo run -q -p lope-cli -- --version` prints `lope X.Y.Z (compat 0.17.3)`.

### Local smoke test

- [ ] **Installer & packaged tarball qualification.** Build `./scripts/cargo build --release`, package `./scripts/package-release.sh`, extract in an isolated prefix + `HOME` (with spaces and symlinked `bin/lope`), run `./install.sh`, verify `lope --version`, `lope docs`, and `lope install --host all`. Do not overwrite modified local skills merely to tick this box. Record any unexecuted installer gate.
- [ ] **Rust workspace & parity pass.** Run `./scripts/cargo test --workspace --locked`, `./scripts/cargo clippy --workspace --all-targets --locked`, `./tools/parity/dump-python && ./tools/parity/dump-rust && ./tools/parity/diff`, and `/usr/bin/python3 -I tools/parity/golden_run.py` across all `tools/parity/corpus/*.json` files.
- [ ] **Tiny negotiate / review smoke.** Verify `lope status` and a bounded live probe against configured validators, plus rollback (`LOPE_FORCE_PYTHON=1` / `lope-py`).
- [ ] **Python compat & package tests pass.** Run `pytest -q`, `ruff check .`, `compileall`, version sync, gitleaks, and `python -m build && twine check && python scripts/check-package.py` for the frozen `0.17.3` compatibility wheel.
- [ ] **Native adapter smoke.** Exercise changed provider paths with an approved bounded fixture. A keyless test is not native-provider proof; a successful fixture is not a universal permission guarantee.

### Commit, verify CI, and tag

- [ ] **Stage only the release files.** `git add Cargo.toml Cargo.lock install CHANGELOG.md .claude-plugin/plugin.json .cursor-plugin/plugin.json gemini-extension.json` plus changed docs/feature/fix files. Never `git add .`.
- [ ] **Commit with a release body.** Subject: `vX.Y.Z — <tagline>`. Body: the CHANGELOG section pasted verbatim (minus the `## X.Y.Z —` header). Use a HEREDOC, not inline `-m`.
- [ ] **Push target release branch first and wait for CI green.** Push the release commit (`git push origin v2` / `main`) and wait until all `.github/workflows/ci.yml` jobs succeed on that exact commit before tagging.
- [ ] **Tag annotated only after CI passes and publication is approved.** `git tag -a vX.Y.Z -m "vX.Y.Z — <tagline>"`, then `git push origin vX.Y.Z`. Lightweight tags don't carry metadata and break GitHub's release UI.

### Release pipeline & artifact verification

`.github/workflows/publish.yml` verifies strict SemVer tag / `lope-cli` version agreement, gates release publication on the full `.github/workflows/ci.yml` suite, builds POSIX release tarballs across macOS (`arm64`, `x86_64`) and Linux (`x86_64`, `aarch64` on `ubuntu-22.04` `glibc >= 2.35` baseline), smoke-tests archives natively / via Rosetta / via QEMU, writes per-tarball `.sha256` sidecars and `SHA256SUMS`, generates Sigstore provenance via `actions/attest-build-provenance`, and publishes the tarballs, checksums, and sidecars to GitHub Releases (`makakoo/lope`). After release, verify `shasum -a 256 -c SHA256SUMS` and `gh attestation verify <tarball> --repo makakoo/lope`.

### Post-push verification

- [ ] **Tag is live on GitHub.** `git ls-remote --tags origin | grep vX.Y.Z` prints the tag.
- [ ] **Live `INSTALL.md` matches.** `curl -sS -o /tmp/live_install.md -w "HTTP %{http_code}\n" https://raw.githubusercontent.com/makakoo/lope/v2/INSTALL.md` returns `HTTP 200` and the file content matches local `INSTALL.md`.
- [ ] **Live `CHANGELOG.md` matches.** Same curl pattern for `CHANGELOG.md`. Spot-check the new section is visible.
- [ ] **Paste-a-prompt install still works.** In a fresh session, paste:
  ```
  Read https://raw.githubusercontent.com/makakoo/lope/v2/INSTALL.md and follow the instructions to install lope on this machine natively.
  ```
  Agent fetches, installs, reports back. For a PATCH release this is a 30-second smoke. For MINOR/MAJOR, run it in at least 2 different host CLIs.
- [ ] **Preservation-aware local sync.** Compare installed/source hashes and diffs. Back up the exact affected files, verify they have not changed since inspection, then synchronize only reviewed owning modules and version surfaces. Preserve local provider config, permissions, credentials, history and modified skills. Use a separate clean checkout for full installer/release verification. Report local-source parity separately from Git ancestry or publication.

### Announce (MINOR and MAJOR only, skip for PATCH unless the fix is user-visible)

- [ ] **Update the Harvey Brain journal** at `~/HARVEY/data/Brain/journals/YYYY_MM_DD.md` with a one-paragraph release note.
- [ ] **Update marketing drafts** at `~/HARVEY/marketing/lope/` if the release ships anything the campaign references (v0.3.0 features, version number in blog posts, etc.). Run the per-file grep: `grep -rn "v0\.[0-9]" ~/HARVEY/marketing/lope/` and fix every hit.
- [ ] **Draft the LinkedIn/Twitter announcement** in `~/HARVEY/marketing/lope/linkedin/` if the release is big enough to warrant one. Follow the existing numbering. Do not auto-send — Sebastian reviews and fires manually.
- [ ] **GitHub release** (optional): `gh release create vX.Y.Z --title "vX.Y.Z — <tagline>" --notes-file <(sed -n '/## X.Y.Z/,/## /p' CHANGELOG.md | sed '$d')` if you want GitHub's release UI populated.

### Rollback (if the release is broken)

- [ ] **Never force-push.** Cut a PATCH release instead (`./scripts/bump-version.sh X.Y.Z+1`) with a CHANGELOG line like `## 0.3.2 — revert the broken change in 0.3.1`. Fixes forward.
- [ ] **If the tag itself is poisoned** and needs removal: tell Sebastian first, delete with `git push --delete origin vX.Y.Z && git tag -d vX.Y.Z`, then cut the corrected release. Never do this silently.

## Hard rules — do not break these

- **Never skip the dual version sync check.** Run `./scripts/check-version.sh` to verify both the v2 product group and `0.17.3` compat pin.
- **Never release without updating CHANGELOG.md.** A release without a CHANGELOG entry is worse than no release — users can't tell what changed.
- **Never tag an unverified commit.** Tags must point to a CI-green release commit on the target release branch (`v2` / `main`).
- **Never commit with `git add .` or `git add -A`.** List the files explicitly.
- **Never force-push to release branches or a tag.** If something is broken, fix forward with a new PATCH.

## Quick reference

```bash
# Example after explicit release approval and all checks above; not an automatic script:
./scripts/bump-version.sh 2.0.1                           # 1. bump product v2
./scripts/check-version.sh                                # 2. verify sync
$EDITOR CHANGELOG.md                                      # 3. add release notes
./scripts/cargo test --workspace --locked                 # 4. verify Rust + parity
git add Cargo.toml Cargo.lock install \
        CHANGELOG.md .claude-plugin/plugin.json \
        .cursor-plugin/plugin.json gemini-extension.json  # 5. stage
git commit                                                # 6. commit (HEREDOC body)
git tag -a v2.0.1 -m "v2.0.1 — <tagline>"                 # 7. tag
git push origin v2                                        # 8a. push approved commit
# Verify green CI for this exact commit, then obtain/confirm publication approval.
git push origin v2.0.1                                    # 8b. publishes GitHub Release tarballs
```

That's it. Memorize nothing — read this file every single time.
