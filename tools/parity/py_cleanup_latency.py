#!/usr/bin/env python3
"""Same D1 workload through Python Lope's supervisor (lope.supervisor)."""
import json, os, subprocess, sys, tempfile, collections, time
src = os.path.realpath(os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope"))
n = int(sys.argv[1]) if len(sys.argv) > 1 else 30
d = tempfile.mkdtemp()
p = os.path.join(d, "provider")
open(p, "w").write("#!/bin/sh\necho answer\nsleep 30 &\nsleep 30 &\nexit 0\n"); os.chmod(p, 0o755)
outcomes = collections.Counter(); walls = []
for i in range(n):
    res = os.path.join(d, "r%d.json" % i)
    spec = json.dumps({"command": [p], "timeout": 30.0}) + "\n"
    t = time.monotonic()
    proc = subprocess.Popen([sys.executable, "-m", "lope.supervisor", res], stdin=subprocess.PIPE, cwd=src,
                            env=dict(os.environ, PYTHONPATH=src, PYTHONDONTWRITEBYTECODE="1"))
    proc.stdin.write(spec.encode()); proc.stdin.flush()
    proc.wait(); proc.stdin.close()
    walls.append(time.monotonic() - t)
    r = json.load(open(res))
    outcomes["%s/%s" % (r["outcome"], r["cleanup_result"])] += 1
walls.sort()
print("python supervisor: n=%d load1=%.1f outcomes=%s wall p50=%.3fs max=%.3fs" % (n, os.getloadavg()[0], dict(outcomes), walls[n // 2], walls[-1]))
