#!/usr/bin/env python3
"""Golden-run harness (NFR14, S0 task 4d, A2).

For each corpus case, runs the Rust `lope` and direct Python Lope
(`PYTHONPATH=<lope-home> python3 -m lope`, the current shim) in identical
sandboxes, then compares exit code, stdout, normalised stderr and a snapshot
of everything written.

Sandbox (rebuilt from scratch at the same path for each side, so absolute
paths match without rewriting):
  <case>/home        HOME (memory.db lives in $HOME/.lope, not $LOPE_HOME)
  <case>/lope-home   LOPE_HOME: symlinks to the tracked top-level entries of
                     the source checkout (code, docs, skills) + a fixture
                     config.json naming mock validators. No real state.
  <case>/work        cwd, with the case's fixture files
  <case>/mockbin     mock validator CLIs, first on PATH

The source checkout is never written to (PYTHONDONTWRITEBYTECODE=1, all state
under the sandbox).

Usage: golden_run.py --lope BIN --corpus FILE [--lope-home DIR] [--only NAME] [--keep DIR]
Exit: 0 all cases match, 1 a mismatch, 2 harness error.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

PYTHON = "/usr/bin/python3"

MOCK_VALIDATOR = r'''#!/bin/sh
# Deterministic offline validator. Reads the prompt on stdin; the answer
# depends only on the validator name and a checksum of the prompt, so
# identical prompts on both sides give identical reports.
name=${1:-mock}
sum=$(cksum | cut -d' ' -f1)
case "$name" in mock-fail) echo "boom: provider failed" >&2; exit 3 ;; esac
case "$LOPE_MOCK_MODE" in
  verdict)
    printf 'Reviewed by %s (prompt %s).\n\n1. [major] Missing error handling in section 2.\n2. [minor] Typo in the introduction.\n\n---VERDICT---\n{"status": "PASS", "confidence": 0.9, "rationale": "mock %s approves"}\n---END---\n' "$name" "$sum" "$name" ;;
  *)
    printf '%s says: 4 (prompt %s)\n' "$name" "$sum" ;;
esac
'''

# Volatile tokens, masked in stdout/stderr and in snapshot file contents.
MASKS = [
    (re.compile(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?"), "<TIMESTAMP>"),
    (re.compile(r"\b\d{8}T\d{6}Z?\b"), "<TIMESTAMP>"),
    (re.compile(r"\b\d{4}_\d{2}_\d{2}\b"), "<DATE>"),
    (re.compile(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b"), "<UUID>"),
    (re.compile(r"\b1[6-9]\d{8}(?:\.\d+)?\b"), "<EPOCH>"),
    (re.compile(r"\b\d+(?:\.\d+)?\s?(?:ms|s)\b"), "<DURATION>"),
    (re.compile(r'"(duration|elapsed|latency|seconds|wall|started|finished|created|updated|pid|pgid)[a-z_]*":\s*[-0-9.e]+'), r'"\1":<N>'),
    (re.compile(r"\bpid[ =:]\s*\d+\b"), "pid <PID>"),
    (re.compile(r"ps:\w{3} \w{3} +\d+ \d{2}:\d{2}:\d{2} \d{4}"), "ps:<START>"),
    (re.compile(r"\b(owner|cpu|rss|deadline)=-?[\d.]+"), r"\1=<N>"),
    (re.compile(r"\b[0-9a-f]{16,64}\b"), "<HEX>"),
]


VOLATILE_INT_KEYS = {"pid", "pgid", "ppid", "pids", "rss_bytes", "duration_ms"}


def mask_json(value, key=None):
    """Mask values that differ between two identical runs: floats (timings,
    epochs), pids, large ints (epochs) and id-like strings. Dict keys that are
    generated ids are masked; colliding entries are kept as a sorted list."""
    if isinstance(value, dict):
        items = [(mask(k), mask_json(v, k)) for k, v in value.items()]
        keys = [k for k, _ in items]
        if len(set(keys)) == len(keys):
            return dict(sorted(items))
        return sorted((json.dumps([k, v], sort_keys=True) for k, v in items))
    if isinstance(value, list):
        return [mask_json(v, key) for v in value]
    if isinstance(value, bool) or value is None:
        return value
    if isinstance(value, float):
        return "<float>"
    if isinstance(value, int):
        return "<int>" if key in VOLATILE_INT_KEYS or abs(value) > 10 ** 9 else value
    if isinstance(value, str):
        return mask(value)
    return value


def mask_content(name, data):
    text = data.decode("utf-8", "replace")
    if name.endswith((".json", ".jsonl")):
        try:
            if name.endswith(".jsonl"):
                return "\n".join(json.dumps(mask_json(json.loads(l)), sort_keys=True) for l in text.splitlines() if l.strip())
            return json.dumps(mask_json(json.loads(text)), sort_keys=True)
        except ValueError:
            pass
    return mask(text)


def mask(text):
    for rx, repl in MASKS:
        text = rx.sub(repl, text)
    return text


def tracked_top_level(src):
    out = subprocess.run(["git", "-C", src, "ls-files"], capture_output=True, text=True, check=True).stdout
    return sorted({line.split("/", 1)[0] for line in out.splitlines() if line})


SETUP_PRELUDE = """
import os, sys, json, time, signal, subprocess
BASE = os.environ["GOLDEN_BASE"]
def spawn(*argv):
    p = subprocess.Popen(list(argv) or ["sleep", "600"], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)
    with open(os.path.join(BASE, "pids.txt"), "a") as f:
        f.write("%d\\n" % p.pid)
    return p
def dead_pid():
    p = subprocess.Popen(["sleep", "600"], start_new_session=True)
    return p
"""


def teardown(base):
    path = os.path.join(base, "pids.txt")
    if not os.path.exists(path):
        return
    for line in open(path).read().split():
        try:
            os.killpg(int(line), 9)
        except (ProcessLookupError, PermissionError, ValueError):
            pass
    os.unlink(path)


def build_sandbox(base, case, src, entries):
    if os.path.exists(base):
        shutil.rmtree(base)
    for d in ("home", "lope-home", "work", "mockbin", "tmp"):
        os.makedirs(os.path.join(base, d))
    lh = os.path.join(base, "lope-home")
    for e in entries:
        os.symlink(os.path.join(src, e), os.path.join(lh, e))
    mock = os.path.join(base, "mockbin", "mock-validator")
    with open(mock, "w") as f:
        f.write(MOCK_VALIDATOR)
    os.chmod(mock, 0o755)
    validators = case.get("validators", ["mock-a", "mock-b"])
    config = {
        "version": 1,
        "validators": validators,
        "primary": validators[0],
        "timeout": 60,
        "parallel": False,
        "providers": [
            {"name": v, "type": "subprocess", "command": [mock, v], "stdin": True, "timeout": 60}
            for v in validators
        ],
    }
    with open(os.path.join(lh, "config.json"), "w") as f:
        json.dump(config, f, indent=2)
    for rel, content in case.get("files", {}).items():
        p = os.path.join(base, "work", rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(content)


def run_side(side, base, case, lope_bin):
    env = {
        "HOME": os.path.join(base, "home"),
        "LOPE_HOME": os.path.join(base, "lope-home"),
        "PATH": os.path.join(base, "mockbin") + ":/usr/bin:/bin:/usr/sbin:/sbin",
        "PYTHONDONTWRITEBYTECODE": "1",
        "LANG": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "TMPDIR": os.path.join(base, "tmp") + "/",
    }
    env.update(case.get("env", {}))
    if side == "rust":
        argv = [lope_bin] + case["argv"]
        env["LOPE_ROUTER_TRACE"] = "1"
    else:
        env["PYTHONPATH"] = env["LOPE_HOME"]
        argv = [PYTHON, "-m", "lope"] + case.get("python_argv", case["argv"])
    if case.get("setup_py"):
        # Fixture setup with Python Lope's own APIs; may start helper
        # processes, whose pids go to <base>/pids.txt for teardown.
        setup_env = dict(env, PYTHONPATH=env["LOPE_HOME"], GOLDEN_BASE=base)
        subprocess.run([PYTHON, "-c", SETUP_PRELUDE + case["setup_py"]], cwd=os.path.join(base, "work"),
                       env=setup_env, check=True, timeout=60)
    stdin = case.get("stdin")
    t0 = time.monotonic()
    p = subprocess.run(
        argv, cwd=os.path.join(base, "work"), env=env,
        input=(stdin or "").encode(), capture_output=True, timeout=case.get("timeout", 120),
    )
    teardown(base)
    stderr = p.stderr.decode("utf-8", "replace")
    route = None
    if side == "rust":
        # The router's trace line is the first stderr line; record and strip it.
        lines = stderr.split("\n")
        trace = [l for l in lines if l.startswith("lope-router: ")]
        if trace:
            route = trace[0].split()[1]
            if any("handed back to python" in l for l in trace):
                route = "handback"
            stderr = "\n".join(l for l in lines if not l.startswith("lope-router: "))
    return {
        "exit": p.returncode,
        "route": route,
        "stdout": p.stdout.decode("utf-8", "replace"),
        "stderr": stderr,
        "seconds": time.monotonic() - t0,
        "snapshot": snapshot(base),
    }


def snapshot(base):
    """Relative path -> masked-content digest, for every file written in the
    sandbox (symlinks into the source checkout are skipped)."""
    snap = {}
    for top in ("home", "lope-home", "work", "tmp"):
        root = os.path.join(base, top)
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if not os.path.islink(os.path.join(dirpath, d))]
            for fn in filenames:
                full = os.path.join(dirpath, fn)
                if os.path.islink(full):
                    continue
                rel = mask(os.path.relpath(full, base))
                with open(full, "rb") as f:
                    data = f.read()
                if fn.endswith((".db", ".db-wal", ".db-shm", ".lock")) or b"\0" in data[:1024]:
                    digest = "binary"  # SQLite pages vary with timing; presence is compared
                else:
                    digest = hashlib.sha256(mask_content(fn, data).encode()).hexdigest()[:16]
                key = rel
                n = 2
                while key in snap:  # masked names may collide (two runs, same masked id)
                    key = "%s#%d" % (rel, n)
                    n += 1
                snap[key] = digest
    return dict(sorted(snap.items()))


# Periodic stderr lines Python Lope prints on a timer while a call is slow.
# Whether one appears depends only on machine speed, so they are dropped
# before comparing (A2 compares *normalised* stderr).
TIMER_LINES = re.compile(r"^heartbeat: .*$\n?", re.M)


def normalise_stream(stream, text):
    stripped = text.strip()
    if stream == "stdout" and stripped[:1] in ("{", "["):
        try:
            return json.dumps(mask_json(json.loads(stripped)), sort_keys=True, indent=1)
        except ValueError:
            pass
    text = mask(text)
    if stream == "stderr":
        text = TIMER_LINES.sub("", text)
    return text


def compare(case, rust, py, default_route=None):
    problems = []
    want_route = case.get("route", default_route)
    if want_route and rust["route"] != want_route:
        problems.append("route: expected %s, rust took %s" % (want_route, rust["route"]))
    if rust["exit"] != py["exit"]:
        problems.append("exit: rust=%s python=%s" % (rust["exit"], py["exit"]))
    for stream in ("stdout", "stderr"):
        if stream == "stdout" and case.get("stdout_contract") == "v2_docs":
            if not py["stdout"].strip():
                problems.append("python docs stdout unexpectedly empty")
            txt = rust["stdout"]
            if len(txt) < 10_000:
                problems.append("v2 docs stdout too short (%d bytes)" % len(txt))
            for heading in (
                "# Lope — Complete Reference",
                "## The modes",
                "## CLI reference",
                "## Environment variables",
                "## Config precedence",
                "## Install",
                "## Hard rules for agents invoking lope",
                "## Where to read more",
            ):
                if heading not in txt:
                    problems.append("v2 docs missing section %r" % heading)
            for forbidden in (
                "Zero Python dependencies",
                "traylinx/lope",
                "PYTHONPATH=",
                "python3 -m lope",
                "super-lope",
            ):
                if forbidden.lower() in txt.lower():
                    problems.append("v2 docs still contains forbidden %r" % forbidden)
            continue
        a, b = normalise_stream(stream, rust[stream]), normalise_stream(stream, py[stream])
        if a != b:
            problems.append("%s differs:\n%s" % (stream, unified(b, a)))
    if rust["snapshot"] != py["snapshot"]:
        problems.append("written state differs:\n%s" % unified(
            json.dumps(py["snapshot"], indent=1), json.dumps(rust["snapshot"], indent=1)))
    for k, v in case.get("expect", {}).items():
        if k == "exit" and rust["exit"] != v:
            problems.append("expected exit %s, got %s" % (v, rust["exit"]))
        if k == "exit_not" and rust["exit"] == v:
            problems.append("exit must not be %s" % v)
        if k == "stdout_contains":
            for s in v:
                if s not in rust["stdout"]:
                    problems.append("stdout lacks %r" % s)
    return problems


def unified(expected, actual):
    import difflib
    lines = difflib.unified_diff(expected.splitlines(), actual.splitlines(), "python", "rust", lineterm="", n=1)
    return "\n".join(list(lines)[:40])


def _default_python_checkout():
    """Locate the read-only Python Lope checkout used to populate parity sandboxes."""
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    for candidate in (
        os.environ.get("LOPE_PYTHON_CHECKOUT"),
        os.environ.get("LOPE_HOME"),
        repo_root,
        os.path.expanduser("~/.lope"),
    ):
        if candidate and os.path.isfile(os.path.join(candidate, "lope", "__init__.py")):
            return os.path.realpath(candidate)
    return repo_root


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--lope", required=True, help="Rust lope binary")
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--lope-home", default=_default_python_checkout(),
                    help="source Python Lope checkout (default $LOPE_PYTHON_CHECKOUT, ~/.lope if git checkout, or repo root)")
    ap.add_argument("--only")
    ap.add_argument("--keep", help="keep sandboxes under this directory")
    ap.add_argument("--show", action="store_true", help="print both sides' raw stdout/stderr")
    ap.add_argument("--attempts", type=int, default=3,
                    help="re-run a mismatching case up to N times (default 3; the matching attempt "
                         "is reported). Guards against timing-only differences under heavy load")
    args = ap.parse_args()
    args.lope = os.path.abspath(args.lope)

    corpus = json.load(open(args.corpus))
    src = os.path.realpath(args.lope_home)
    entries = tracked_top_level(src)
    # Resolve the root: on macOS tempfile gives /var/folders/..., and /var is a
    # symlink. Python Lope's jobs.py:is_owned_path then rejects every owned
    # path (its ancestor walk overshoots the resolved root and meets the /var
    # symlink), so each run with validator calls ends "owned scratch cleanup
    # failure", exit 1. That bug is recorded in docs/evidence/s0-router.txt;
    # the corpus exercises the normal success path instead.
    root = os.path.realpath(args.keep or tempfile.mkdtemp(prefix="lope-golden-"))
    failures = 0
    total = 0
    print("golden-run: rust=%s python=%s source=%s" % (args.lope, PYTHON, src))
    try:
        for case in corpus["cases"]:
            if args.only and case["name"] != args.only:
                continue
            total += 1
            base = os.path.join(root, case["name"])
            if case.get("symlink_root"):
                # Same sandbox, but reached through a symlinked ancestor.
                real = os.path.join(root, "_real")
                link = os.path.join(root, "_link")
                os.makedirs(real, exist_ok=True)
                if not os.path.islink(link):
                    os.symlink(real, link)
                base = os.path.join(link, case["name"])
            for attempt in range(1, args.attempts + 1):
                build_sandbox(base, case, src, entries)
                rust = run_side("rust", base, case, args.lope)
                build_sandbox(base, case, src, entries)
                py = run_side("python", base, case, args.lope)
                problems = compare(case, rust, py, corpus.get("default_route"))
                if not problems:
                    break
            status = "PASS" if not problems else "FAIL"
            failures += bool(problems)
            flaky_python = any("closeout warning" in r["stderr"] for r in (rust, py))
            print("%-4s %-34s %-8s exit=%-3s files=%-3d rust=%.2fs python=%.2fs attempt=%d%s" % (
                status, case["name"], rust["route"], rust["exit"], len(rust["snapshot"]), rust["seconds"], py["seconds"],
                attempt, " (python closeout warning seen)" if flaky_python else ""))
            for p in problems:
                print("     " + p.replace("\n", "\n     "))
            if args.show:
                for side, r in (("rust", rust), ("python", py)):
                    for stream in ("stdout", "stderr"):
                        print("---- %s %s ----\n%s" % (side, stream, r[stream]))
    finally:
        if not args.keep:
            shutil.rmtree(root, ignore_errors=True)
    print("golden-run: %d/%d cases identical" % (total - failures, total))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
