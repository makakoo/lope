#!/usr/bin/env python3
"""NFR2: `lope version` startup p50/p95 over N runs (default 20), plus router
overhead on a delegated invocation (Rust `lope version --help` vs direct
Python, whose only difference is the router + exec).

Usage: bench-version.py RUST_LOPE [--runs N]
Prints a table and the 1-minute load average (results under heavy load are
not comparable to the NFR2 budget and are flagged).
"""
import os
import statistics
import subprocess
import sys
import time

PY = ["/usr/bin/python3", "-m", "lope"]


def timed(argv, env, runs):
    out = []
    for _ in range(runs):
        t0 = time.perf_counter()
        subprocess.run(argv, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        out.append((time.perf_counter() - t0) * 1000.0)
    return out


def pct(xs, p):
    xs = sorted(xs)
    k = (len(xs) - 1) * p
    lo, hi = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def main():
    rust = os.path.abspath(sys.argv[1])
    runs = int(sys.argv[sys.argv.index("--runs") + 1]) if "--runs" in sys.argv else 20
    home = os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope")
    env = {"HOME": os.environ["HOME"], "PATH": "/usr/bin:/bin", "LOPE_HOME": home, "PYTHONPATH": home,
           "PYTHONDONTWRITEBYTECODE": "1"}
    rows = [
        ("rust  lope version (native)", [rust, "version"]),
        ("baseline /usr/bin/true (spawn floor)", ["/usr/bin/true"]),
        ("python lope version", PY + ["version"]),
        ("rust  lope version --help (delegated)", [rust, "version", "--help"]),
        ("python lope version --help", PY + ["version", "--help"]),
    ]
    load = os.getloadavg()[0]
    print("runs=%d  load1=%.1f  cpus=%d%s" % (runs, load, os.cpu_count(),
          "  ** HEAVY LOAD: not comparable to NFR2 budgets **" if load > os.cpu_count() else ""))
    results = {}
    for name, argv in rows:
        timed(argv, env, 2)  # warm caches
        xs = timed(argv, env, runs)
        results[name] = xs
        print("%-40s p50=%8.1f ms  p95=%8.1f ms  min=%8.1f ms" % (name, pct(xs, 0.5), pct(xs, 0.95), min(xs)))
    deleg, direct = results[rows[3][0]], results[rows[4][0]]
    print("router overhead on delegation: p50 diff %+.1f ms, min diff %+.1f ms (budget <= 15 ms)"
          % (pct(deleg, 0.5) - pct(direct, 0.5), min(deleg) - min(direct)))
    floor = results[rows[1][0]]
    print("native version above spawn floor: p50 %+.1f ms" % (pct(results[rows[0][0]], 0.5) - pct(floor, 0.5)))


if __name__ == "__main__":
    main()
