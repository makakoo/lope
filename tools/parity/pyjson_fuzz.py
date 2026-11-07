#!/usr/bin/env python3
"""Differential fuzz: lope_core::pyjson vs CPython json.dumps (4 flavours)."""
import json, random, subprocess, sys, struct

rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 7)

def rfloat():
    k = rng.random()
    if k < 0.3:
        return struct.unpack("d", struct.pack("Q", rng.getrandbits(64)))[0]
    if k < 0.6:
        return rng.uniform(-1e6, 1e6)
    if k < 0.8:
        return rng.choice([0.0, -0.0, 1e16, 1e-5, 1e-4, 9999999999999998.0, 1794057125.7847402, 0.1, 5e-324])
    return rng.random() * 10 ** rng.randint(-30, 30)

def rstr():
    alphabet = ["a", "Z", "0", " ", "\"", "\\", "/", "\n", "\t", "\x01", "\x1f", "\x7f", "\x80", "é", " ", "퟿", "\U0001f600", "\u0000"]
    return "".join(rng.choice(alphabet) for _ in range(rng.randint(0, 8)))

def rvalue(depth=0):
    k = rng.random()
    if depth > 3 or k < 0.5:
        return rng.choice([None, True, False, rng.randint(-2**63, 2**63 - 1), rng.randint(-100, 100), rfloat(), rstr()])
    if k < 0.75:
        return [rvalue(depth + 1) for _ in range(rng.randint(0, 4))]
    return {rstr(): rvalue(depth + 1) for _ in range(rng.randint(0, 4))}

docs = []
while len(docs) < int(sys.argv[1]) if len(sys.argv) > 1 else 2000:
    v = rvalue()
    s = json.dumps(v)
    if "NaN" in s or "Infinity" in s:
        continue
    docs.append(v)
inp = "\n".join(json.dumps(d) for d in docs) + "\n"
exe = "target/debug/examples/pyjson_dump"
out = subprocess.run([exe], input=inp.encode(), capture_output=True, check=True).stdout.decode().splitlines()
bad = 0
for d, line in zip(docs, out):
    want = [json.dumps(d), json.dumps(d, sort_keys=True, separators=(",", ":")),
            json.dumps(d, indent=2, sort_keys=True), json.dumps(d, indent=2)]
    got = [g.replace("\\n", "\n") if False else g for g in line.split("\t")]
    want = [w.replace("\n", "\\n") for w in want]
    if got != want:
        bad += 1
        if bad <= 5:
            for i, (g, w) in enumerate(zip(got, want)):
                if g != w:
                    print("MISMATCH flavour", i, "\n  py:", w[:300], "\n  rs:", g[:300])
print("pyjson-fuzz: %d docs, %d mismatches" % (len(docs), bad))
sys.exit(1 if bad else 0)
