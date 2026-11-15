#!/usr/bin/env python3
"""A3 + A4 (S1): config.json round trip and run-registry interop, Rust <-> Python.

A3: for every fixture config, Rust load+save must be byte-identical to
    Python load+save, Python must load what Rust wrote, and configs Python
    rejects must be rejected by Rust too. Fixtures include the *shape* of the
    live ~/.lope/config.json (copied to a temp dir, never printed).
A4: manifests written by either side are listed, reaped and closed by the
    other side with identical results; 3 dead runs are pruned and 1 live run
    is left untouched; Python tolerates extra (Rust-only) manifest fields.
"""
import json, os, shutil, subprocess, sys, tempfile, time
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.realpath(os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope"))
sys.path.insert(0, SRC)
EX = os.path.join(ROOT, "target", "debug", "examples")
fails = []

def check(cond, what):
    print(("PASS " if cond else "FAIL ") + what)
    if not cond:
        fails.append(what)

SECTIONS = set(sys.argv[1:]) or {"a3", "a4"}

# ---------------- A3 ----------------
from lope import config as pycfg
fixtures = {} if "a3" not in SECTIONS else {
    "minimal": {"version": 1, "validators": ["a"], "primary": "a"},
    "full": {"version": 1, "validators": ["a", "b"], "primary": "b", "timeout": 300, "parallel": False,
             "providers": [{"name": "a", "type": "subprocess", "command": ["x", "{prompt}"], "stdin": False, "extra": [1, 2.5, None]}],
             "learned_adapters": {"claude": {"argv_template": ["claude", "-p"], "stdin_mode": "pipe", "stdout_parser": "json:a.b",
                                              "timestamp": 1794057125.7847402, "source_cli": "codex", "confidence": 0.9}},
             "run_timeout": 1200, "allow_unbounded_run": True, "max_calls": 7, "max_input_bytes": 1000, "max_output_bytes": 2000,
             "request_policy": "chunk", "max_chunks": 3},
    "unknown_keys_dropped": {"version": 1, "validators": [], "primary": "", "super_lope": {"x": 1}, "zzz": True},
    "bool_as_int": {"version": True, "validators": [1, "b"], "primary": "", "timeout": True, "max_calls": True},
    "defaults_omitted": {"version": 1, "validators": [], "primary": "", "max_calls": 96, "max_chunks": 32, "request_policy": "auto",
                         "allow_unbounded_run": False, "run_timeout": None},
    "learned_partial": {"version": 1, "validators": [], "primary": "", "learned_adapters": {"a": {"argv_template": ["x"], "timestamp": 5, "confidence": "0.5"},
                        "bad": {"argv_template": [1]}, "notdict": 3}},
    "unicode": {"version": 1, "validators": ["é"], "primary": "\U0001f600", "providers": [{"name": "ü\n\t\"q\""}]},
    "float_version": {"version": 1.0, "validators": [], "primary": ""},
    # rejected by Python
    "rej_version2": {"version": 2, "validators": [], "primary": ""},
    "rej_no_version": {"validators": [], "primary": ""},
    "rej_float_timeout": {"version": 1, "validators": [], "primary": "", "timeout": 9.5},
    "rej_policy": {"version": 1, "validators": [], "primary": "", "request_policy": "fast"},
    "rej_zero_calls": {"version": 1, "validators": [], "primary": "", "max_calls": 0},
    "rej_list": [1, 2],
    "rej_bad_learned_float": {"version": 1, "validators": [], "primary": "", "learned_adapters": {"a": {"argv_template": [], "timestamp": "x"}}},
}
tmp = tempfile.mkdtemp(prefix="s1-a3-")
live = os.path.join(SRC, "config.json")
if os.path.exists(live) and "a3" in SECTIONS:
    fixtures["live_shape"] = json.load(open(live))
for name, data in fixtures.items():
    src = os.path.join(tmp, name + ".json")
    with open(src, "w") as f:
        json.dump(data, f)
    py_out, rs_out = os.path.join(tmp, name + ".py.json"), os.path.join(tmp, name + ".rs.json")
    try:
        cfg = pycfg.load(src)
    except ValueError as exc:
        # Python Lope bug (finding F7): _hydrate_cfg's float() is unguarded,
        # so load() raises instead of returning None. Rust returns None
        # (treats the file as rejected) rather than crashing.
        rs = subprocess.run([os.path.join(EX, "config_roundtrip"), src, rs_out], capture_output=True, text=True).stdout.strip()
        check(rs == "NONE", "A3 %s: Python raises ValueError (F7); Rust rejects the file instead of crashing" % name)
        continue
    if cfg is not None:
        pycfg.save(cfg, py_out)
    try:
        rs = subprocess.run([os.path.join(EX, "config_roundtrip"), src, rs_out], capture_output=True, text=True, check=True).stdout.strip()
    except subprocess.CalledProcessError as e:
        check(False, "A3 %s: rust crashed: %s" % (name, e.stderr[-300:]))
        continue
    if cfg is None:
        check(rs == "NONE", "A3 %s: both reject" % name)
        continue
    same = rs == "SAVED" and open(py_out, "rb").read() == open(rs_out, "rb").read()
    check(same, "A3 %s: Rust save is byte-identical to Python save" % name)
    check(pycfg.load(rs_out) is not None, "A3 %s: Python loads the Rust-written config" % name)
    st = os.stat(rs_out).st_mode & 0o777
    check(st == os.stat(py_out).st_mode & 0o777, "A3 %s: same file mode (%o)" % (name, st))
shutil.rmtree(tmp)

# ---------------- A4 ----------------
if "a4" not in SECTIONS:
    print("s1-interop: %d failures" % len(fails))
    sys.exit(1 if fails else 0)
home = os.path.realpath(tempfile.mkdtemp(prefix="s1-a4-"))
os.environ["LOPE_HOME"] = home
from lope.jobs import RunRegistry
py = RunRegistry()

def rs(*args):
    out = subprocess.run([os.path.join(EX, "registry_ops")] + list(args), capture_output=True, text=True,
                         env=dict(os.environ, LOPE_HOME=home))
    if out.returncode != 0:
        raise RuntimeError(out.stderr[-500:])
    return json.loads(out.stdout)

def sleeper():
    return subprocess.Popen(["sleep", "600"], start_new_session=True)

def normal(rows):
    keep = ("run_id", "mode", "state", "classification", "owner_live", "action", "result", "calls", "cleanup", "reason")
    def strip(x):
        if isinstance(x, dict):
            return {k: strip(v) for k, v in sorted(x.items()) if k not in ("age_seconds", "deadline_remaining_seconds", "resources", "heartbeat_at", "at", "ended_at")}
        if isinstance(x, list):
            return [strip(v) for v in x]
        return x
    return sorted((json.dumps(strip({k: r.get(k) for k in keep if k in r}), sort_keys=True) for r in rows))

live_owner = sleeper()
dead = [sleeper() for _ in range(3)]
# Rust writes 1 live + 2 dead, Python writes 1 dead (with owned scratch)
rs("start", "rs-live", str(live_owner.pid), "3600")
for i, p in enumerate(dead[:2]):
    rid = "rs-dead-%d" % i
    rs("start", rid, str(p.pid))
    wd = os.path.join(home, "runs", "work", rid)
    f = os.path.join(wd, "scratch-%d.txt" % i)
    open(f, "w").write("x")
    rs("call", rid, "c%d" % i, f, "finished")
py.start_run("ask", run_id="py-dead-0", owner_pid=dead[2].pid)
f = os.path.join(str(py.run_work_dir("py-dead-0")), "s.txt"); open(f, "w").write("y")
py.register_call("py-dead-0", {"call_id": "c9", "validator": "mock-b", "state": "finished", "owned_paths": [f]})
rs("extra", "rs-live")  # Rust-only fields on a live manifest
for p in dead:
    p.kill(); p.wait()

py_rows, rs_rows = py.list_active(), rs("list")
check(normal(py_rows) == normal(rs_rows), "A4 list: Rust and Python list each other's manifests identically (%d rows)" % len(py_rows))
check(any(r.get("super_lope") for r in py_rows), "A4 extra fields: Python lists a manifest carrying Rust-only fields")
py_dry = py.reap_all(dry_run=True)
rs_dry = rs("reap", "--dry-run")
check(normal(py_dry) == normal(rs_dry), "A4 reap --dry-run: identical plans")
res = rs("prune")  # Rust prunes, including the Python-written dead run
actions = {r["run_id"]: r["action"] for r in res}
check(actions == {"rs-live": "refused", "rs-dead-0": "reaped", "rs-dead-1": "reaped", "py-dead-0": "reaped"},
      "A4 prune: 3 dead pruned, 1 live untouched (%s)" % actions)
left = [r["run_id"] for r in py.list_active()]
check(left == ["rs-live"], "A4 prune: Python sees only the live run left")
check(not os.listdir(os.path.join(home, "runs", "work")) or os.listdir(os.path.join(home, "runs", "work")) == ["rs-live"],
      "A4 prune: scratch of dead runs removed")
completed = sorted(os.listdir(os.path.join(home, "runs", "completed")))
check(len(completed) == 3, "A4 prune: 3 completed manifests written")
for c in completed:
    d = json.load(open(os.path.join(home, "runs", "completed", c)))
    check(d["state"] == "reaped" and d["cleanup_result"] == "clean", "A4 completed manifest %s readable by Python, state=reaped" % c.split("-", 1)[1])
# Python closes the Rust-written live run (with extra fields) after the owner exits normally
py.update_call if False else None
rs("call", "rs-live", "c-live", os.path.join(home, "runs", "work", "rs-live", "x.txt"), "finished")
open(os.path.join(home, "runs", "work", "rs-live", "x.txt"), "w").write("z")
out = py.close_run("rs-live")
check(out["action"] == "closed", "A4 close: Python closes a Rust-written run with Rust-only fields (%s)" % out["action"])
live_owner.kill(); live_owner.wait()
# reverse: Rust closes a Python-written run
p2 = sleeper()
py.start_run("vote", run_id="py-live", owner_pid=p2.pid)
out = rs("close", "py-live")
check(out["action"] == "closed", "A4 close: Rust closes a Python-written run")
p2.kill(); p2.wait()
check(py.list_active() == [], "A4 end: registry empty")
# byte format: Rust and Python write identical bytes for the same manifest content
m = json.load(open(os.path.join(home, "runs", "completed", sorted(os.listdir(os.path.join(home, "runs", "completed")))[-1])))
py._atomic_write(__import__("pathlib").Path(home) / "py-bytes.json", m)
rs_bytes_path = os.path.join(home, "runs", "completed", sorted(os.listdir(os.path.join(home, "runs", "completed")))[-1])
check(open(os.path.join(home, "py-bytes.json"), "rb").read() == open(rs_bytes_path, "rb").read(),
      "A4 bytes: a Rust-written manifest is byte-identical to Python's json.dump of the same content")
shutil.rmtree(home)
print("s1-interop: %d failures" % len(fails))
sys.exit(1 if fails else 0)
