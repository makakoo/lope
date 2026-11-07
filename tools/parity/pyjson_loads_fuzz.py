#!/usr/bin/env python3
"""Differential fuzz: lope_core::pyjson::loads_py vs CPython json.loads,
including the exact JSONDecodeError text for malformed input."""
import json, random, subprocess, sys
rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 5)
PIECES = ['{', '}', '[', ']', ',', ':', '"', '"a"', '"k":', '1', '-', '-0', '0.5', '1e3', '1E+2', '2.', '.5', '01', 'true', 'tru',
          'null', 'nul', 'false', ' ', '\n', '\t', '\\', '\\n', '\\u00e9', '\\ud83d\\ude00', '\\x', '\\u12', 'é', 'x', '\x01', '""', '"\\"']
docs = []
for _ in range(int(sys.argv[1]) if len(sys.argv) > 1 else 4000):
    if rng.random() < 0.3:
        v = {"a": [1, 2.5, None, True, "sé"], "b": {"c": "d"}}
        t = json.dumps(v, indent=rng.choice([None, 2]))
        k = rng.randint(0, len(t))
        docs.append(t[:k] + rng.choice(PIECES) + t[k + rng.randint(0, 2):])
    else:
        docs.append("".join(rng.choice(PIECES) for _ in range(rng.randint(0, 10))))
out = subprocess.run(["target/debug/examples/pyjson_loads"], input=("\n".join(json.dumps(d) for d in docs) + "\n").encode(),
                     capture_output=True, check=True).stdout.decode().splitlines()
bad = 0
for d, got in zip(docs, out):
    try:
        v = json.loads(d)
        s = json.dumps(v)
        if "NaN" in s or "Infinity" in s or "\\ud8" in s or "\\udc" in s or "\\udb" in s or "\\udf" in s:
            continue
        want = "OK " + s
    except json.JSONDecodeError as e:
        want = "ERR " + str(e)
    except RecursionError:
        continue
    if got != want:
        bad += 1
        if bad <= 8:
            print("MISMATCH %r\n  py: %s\n  rs: %s" % (d, want, got))
print("pyjson-loads-fuzz: %d docs, %d mismatches" % (len(docs), bad))
sys.exit(1 if bad else 0)
