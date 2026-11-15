# tools/parity — Python Lope parity tooling (NFR14)

Parity is defined by these machine captures and comparisons, never by prose.

| Tool | What it does | Acceptance |
|---|---|---|
| `dump-python [--update]` | Runs `dump_python.py` under a scrubbed env. It patches `ArgumentParser.parse_args` and calls `lope.cli.main()`, so it captures the real argparse tree without running a command or writing anything. Output: `out/python.json`. Fails if the live tree differs from the copy compiled into `lope-compat` (`crates/lope-compat/grammar/argparse-python-lope.json`); `--update` refreshes that copy. | A1 |
| `dump-rust` | Builds and runs `lope-parity-dump`, which walks the clap tree (never the capture) and writes its parse-relevant projection to `out/rust.json`. | A1 |
| `diff` | Projects the argparse capture into the same schema and compares commands, aliases, options (kind, arity, type, choices, required), positionals, mutually exclusive groups and abbreviation support. Additive differences must be listed with a reason in `allowlist.json`. | A1 |
| `golden_run.py` | Runs each case in `corpus/*.json` through the Rust `lope` and through direct Python (`PYTHONPATH=… python3 -m lope`) in identical sandboxes: fake HOME, LOPE_HOME with symlinked code and a fixture config naming offline mock validators, and a fixture cwd. Compares exit code, stdout, masked stderr and a masked snapshot of every file written. | A2 (`cargo test -p lope-cli --test delegation_fidelity`) |

```sh
tools/parity/dump-python && tools/parity/dump-rust && tools/parity/diff        # A1
/usr/bin/python3 -I tools/parity/golden_run.py --lope target/release/lope \
    --corpus tools/parity/corpus/s0-delegation.json [--only CASE] [--keep DIR] [--show]
lope-parity-dump --coverage     # native coverage table + fingerprint
```

Notes:
- `$LOPE_HOME` (default `~/.lope`) is the Python checkout under test. Nothing in it is written: the sandboxes symlink its tracked top-level entries, and `PYTHONDONTWRITEBYTECODE=1` is set.
- Python Lope keeps `memory.db` in `$HOME/.lope/`, not in `$LOPE_HOME`, so the sandbox HOME is part of the snapshot.
- Sandboxes live under a fully resolved path. If any ancestor of `$LOPE_HOME` is a symlink (e.g. macOS `/var` → `/private/var`), Python Lope's `jobs.py:is_owned_path` rejects its own scratch paths, and every run with validator calls exits 1 with `owned scratch cleanup failure`. The `symlinked-lope-home-closeout-bug` case pins that behaviour on purpose.
- Python Lope prints periodic `heartbeat:` lines on stderr from a timer thread (`progress.py:105`). They are dropped before stderr is compared. A mismatching case is re-run up to `--attempts` times (default 3), and the report says which attempt matched.
