#!/bin/sh
# Assemble docs/evidence/s0-router.txt from the machine outputs in
# tools/parity/out/ (A1, A2, fuzz, bench) plus the coverage table.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/tools/parity/out"
ev="$root/docs/evidence/s0-router.txt"
{
echo "# S0 evidence — router, normalisation, parity tooling"
echo "generated $(date -u +%Y-%m-%dT%H:%M:%SZ) by scripts/s0-evidence.sh"
echo "baseline: Python Lope at ~/.lope, PROVISIONAL (PL-1 open; see pins.txt)"
echo
echo "## Build"
echo "$("$root/scripts/cargo" --version) / $(PATH=$HOME/.cargo/bin:$PATH rustc --version)"
echo "target/release/lope: $(wc -c < "$root/target/release/lope" | tr -d ' ') bytes, $(file -b "$root/target/release/lope" | cut -d, -f1)"
echo "$("$root/target/release/lope" --version)"
echo
echo "## Native coverage table"
"$root/target/release/lope-parity-dump" --coverage
echo
echo "## A1 — argparse vs clap (tools/parity/dump-python && dump-rust && diff)"
cat "$out/a1.txt"
echo
echo "## A2 — delegation fidelity (cargo test -p lope-cli --test delegation_fidelity)"
grep -E '^(golden-run|PASS|FAIL|     )|test result|cargo-test-exit' "$out/a2-cargo-test.txt"
echo
echo "## DEC-11 normalisation fuzz (tools/parity/normalise_fuzz.py, real argparse oracle)"
cat "$out/fuzz.txt"
echo
echo "## Unit and router tests (scripts/cargo test --workspace, excluding A2)"
cat "$out/unit-tests.txt"
echo
echo "## NFR2 startup (scripts/bench-version.py)"
cat "$out/bench.txt"
echo "(Measured with Python subprocess timing on a loaded machine. Lope's own cost is the"
echo " difference from the /usr/bin/true floor. The absolute NFR2 budgets and the router"
echo " overhead budget need a re-measure on a quiet machine.)"
cat <<'TXT'

## Findings (handed to later sprints)
F1  Python Lope closeout bug, committed code (jobs.py:507-522 is_owned_path). If any
    ancestor of $LOPE_HOME is a symlink (macOS /var -> /private/var), every run with
    validator calls ends "owned scratch cleanup failure" and exits 1 after successful
    calls. Pinned by A2 case symlinked-lope-home-closeout-bug; proposed fix PL-5
    (Spec 04 §8). ~/.lope on this machine has no symlinked ancestor, so it is unaffected.
    S1/S2 must decide whether native Rust reproduces or fixes it.
F2  memory.db is at $LOPE_MEMORY_DB, else $HOME/.lope/memory.db, independent of $LOPE_HOME
    (memory.py:44-46, 75-82). Matters for S4 (FR27, A12) and for sandboxed tests.
F3  An empty LOPE_HOME is treated differently across Python modules: `or` in
    jobs.py/runlock.py vs `.get(default)` in config/journal/latency/trust/auditor.
    S1 needs a decision.
F4  --consensus and --structured are two store_true actions sharing dest `consensus`,
    not argparse aliases. Equivalent behaviour; the clap grammar mirrors it.
F5  Python Lope prints timer-driven "heartbeat:" lines on stderr (progress.py:105);
    A2 drops them before comparing.
F6  Build: rustup 1.29 no longer puts the toolchain first when it runs cargo, and
    Homebrew rustc 1.89 is earlier on PATH. scripts/cargo fixes this.

## Open before the S0 gate can close
- PL-1 (Sebastian): commit/tag ~/.lope. Then: tools/parity/dump-python --update,
  regenerate docs/reference/*, re-run A1/A2, regenerate this file.
- NFR15 Glimmer baseline: not run (see pins.txt). Needs an OK or a fake-lope run.
- NFR2 re-measure on a quiet machine.
TXT
} > "$ev"
echo "wrote $ev"
