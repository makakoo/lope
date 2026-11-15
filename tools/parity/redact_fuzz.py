#!/usr/bin/env python3
"""Differential fuzz: lope_core::redaction vs lope.redaction.redact_text."""
import json, os, random, subprocess, sys
sys.path.insert(0, os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope"))
from lope.redaction import redact_text
rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 3)
TOK = ["sk-", "sk-abcdefgh12", "ghp_", "gho_abcdefghijklmn", "Bearer ", "bearer", "BEARER abcdefghijk/=", "api_key", "API-KEY",
       "x-api-key", "authorization", "token", "access_token", "secret", "Secret", ": ", "=", " = ", "\"", "'",
       "ABCDEFGHIJKLMNOPQRSTUV", "abc.def~ghi+jkl/mno=", "0123456789", "-----BEGIN RSA PRIVATE KEY-----",
       "-----END RSA PRIVATE KEY-----", "-----BEGIN PRIVATE KEY-----", "-----END PRIVATE KEY-----",
       "-----BEGIN OPENSSH PRIVATE KEY-----", "-----END OPENSSH PRIVATE KEY-----", "\n", " ", "x", "_", "-", "é", "ß", "K", "K"]
cases = ["".join(rng.choice(TOK) for _ in range(rng.randint(1, 12))) for _ in range(int(sys.argv[1]) if len(sys.argv) > 1 else 3000)]
out = subprocess.run(["target/debug/examples/redact"], input=("\n".join(json.dumps(c) for c in cases) + "\n").encode(),
                     capture_output=True, check=True).stdout.decode().splitlines()
bad = 0
for c, line in zip(cases, out):
    got, want = json.loads(line), redact_text(c)
    if got != want:
        bad += 1
        if bad <= 5:
            print("MISMATCH\n  in: %r\n  py: %r\n  rs: %r" % (c, want, got))
print("redact-fuzz: %d cases, %d mismatches" % (len(cases), bad))
sys.exit(1 if bad else 0)
