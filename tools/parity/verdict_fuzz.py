#!/usr/bin/env python3
"""Differential fuzz: Rust verdict parser / diagnostics redaction / output
validity vs Python Lope (parse_opencode_verdict, redact_diagnostic, classify_output)."""
import json, os, random, subprocess, sys
sys.path.insert(0, os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope"))
os.environ.pop("LOPE_EVIDENCE_GATE", None)
from lope.validators import parse_opencode_verdict
from lope.diagnostics import redact_diagnostic
from lope.output_validity import classify_output
rng = random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 11)
PIECES = ["---VERDICT---\n", "\n---END---", "status: PASS\n", "status: needs_fix\n", "status: FAIL\n", "status: maybe\n",
          "confidence: 0.9\n", "confidence: 0.5\n", "confidence: 1.2.3\n", "rationale: looks fine\n", "rationale: verified at a.py:12\n",
          "required_fixes:\n", "- fix the thing\n", "- (empty)\n", "* star item\n", "nice_to_have:\n", "- polish\n",
          '{"status": "PASS", "confidence": 0.95, "rationale": "tests passed"}', '{"status": "pass", "confidence": 0.6, "rationale": "ok"}',
          '```json\n{"status": "FAIL", "confidence": 1, "rationale": "broken", "required_fixes": ["a", "empty b"]}\n```\n',
          "```\ncode\n```\n", "$ make test\n", "test_foo\n", "exit code 0\n", "prose ", "\n", "  ",
          "Bearer abc.def ", "api_key=xyz ", "sk-abcdefghij ", "<think>secret\n", "</think>", "https://u:p@host/ ",
          "-----BEGIN RSA PRIVATE KEY-----\nzz\n", "-----END RSA PRIVATE KEY-----", "CLEAN", '{"tool_calls": [1]}',
          '{"response": "hi"}', "<tool_use x>", '"thinking": 1\n']
cases = ["".join(rng.choice(PIECES) for _ in range(rng.randint(0, 9))) for _ in range(int(sys.argv[1]) if len(sys.argv) > 1 else 3000)]
out = subprocess.run(["target/debug/examples/verdict_parse"], input=("\n".join(json.dumps(c) for c in cases) + "\n").encode(),
                     capture_output=True, check=True).stdout.decode().splitlines()
bad = 0
for c, line in zip(cases, out):
    got = json.loads(line)
    v = parse_opencode_verdict(c, validator_name="mock", fallback_duration=0.0)
    want = {"status": v.status.value, "confidence": v.confidence, "rationale": v.rationale,
            "required_fixes": v.required_fixes, "nice_to_have": v.nice_to_have, "evidence": v.evidence_gate_triggered,
            "redacted": redact_diagnostic(c), "substantive": classify_output(c).substantive}
    if got != want:
        bad += 1
        if bad <= 5:
            diff = {k: (want[k], got[k]) for k in want if want[k] != got.get(k)}
            print("MISMATCH %r\n  %r" % (c, diff))
print("verdict-fuzz: %d cases, %d mismatches" % (len(cases), bad))
sys.exit(1 if bad else 0)
