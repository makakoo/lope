#!/usr/bin/env python3
"""Differential fuzz of DEC-11 argv normalisation against the real argparse.

Properties checked per random argv:
1. Safety: normalisation only changes (or rejects) an argv that Python Lope
   itself rejects. An argv Python already accepts reaches Python unchanged.
2. negotiate: the rewritten argv means exactly what the original means to an
   argparse in which `--rounds` really is an option string of `--max-rounds`
   (same namespace, or both rejected).
3. review: a rewritten argv that Python accepts has `--consensus` set and a
   `file` other than the consumed mode token.

The Rust side is observed through the real binary: LOPE_PYTHON points at a
stub that prints the argv it was exec'd with.

Usage: normalise_fuzz.py RUST_LOPE [--cases N] [--seed S]
"""
import argparse
import contextlib
import io
import json
import os
import random
import subprocess
import sys
import tempfile

REVIEW_POOL = ["x.md", "y.md", "quorum", "quorum", "unanimous", "any", "--consensus", "--consensus",
               "--structured", "--cons", "--consensus=quorum", "--focus", "--focus=a", "-o", "-oout.md",
               "out.md", "--format", "markdown", "--json", "--", "-1", "--min-consensus", "0.5"]
NEGOTIATE_POOL = ["goal", "--rounds", "--rounds", "3", "--rounds=2", "--max-rounds", "4", "--context",
                  "a b", "--domain", "engineering", "--", "-1", "--max-r", "--out", "o.md"]


def python_parser():
    sys.path.insert(0, os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope"))
    os.environ.pop("LOPE_IMPLEMENTATION_DEPTH", None)
    import lope.cli

    class Stop(Exception):
        pass

    box = {}
    orig = argparse.ArgumentParser.parse_args

    def cap(self, args=None, ns=None):
        if self.prog == "lope":
            box["p"] = self
            raise Stop()
        return orig(self, args, ns)

    argparse.ArgumentParser.parse_args = cap
    try:
        lope.cli.main()
    except Stop:
        pass
    finally:
        argparse.ArgumentParser.parse_args = orig
    return box["p"]


def with_rounds_alias(parser):
    """Deep-copy the parser and make `--rounds` a real option string of
    negotiate's `--max-rounds` action (the DEC-11 meaning)."""
    import copy
    p = copy.deepcopy(parser)
    sub = next(a for a in p._actions if isinstance(a, argparse._SubParsersAction))
    neg = sub.choices["negotiate"]
    act = next(a for a in neg._actions if a.dest == "max_rounds")
    act.option_strings.append("--rounds")
    neg._option_string_actions["--rounds"] = act
    return p


def oracle(parser, argv):
    try:
        with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            return vars(parser.parse_args(argv))
    except SystemExit:
        return None


def rust_normalise(lope, stub, argv):
    env = dict(os.environ, LOPE_PYTHON=stub)
    env.pop("LOPE_IMPLEMENTATION_DEPTH", None)
    p = subprocess.run([lope] + argv, env=env, capture_output=True, text=True)
    if p.returncode == 2 and "error:" in p.stderr:
        return None  # rejected by the router
    if p.returncode != 0:
        raise RuntimeError("unexpected router result %r: %s" % (argv, p.stderr))
    return json.loads(p.stdout)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("lope")
    ap.add_argument("--cases", type=int, default=300)
    ap.add_argument("--seed", type=int, default=20261107)
    a = ap.parse_args()
    rng = random.Random(a.seed)
    parser = python_parser()
    aliased = with_rounds_alias(parser)
    with tempfile.TemporaryDirectory() as d:
        stub = os.path.join(d, "argv-stub")
        with open(stub, "w") as f:
            f.write("#!/usr/bin/python3 -I\nimport json, sys\nprint(json.dumps(sys.argv[3:]))\n")
        os.chmod(stub, 0o755)
        stats = {"cases": 0, "unchanged": 0, "rewritten": 0, "rejected": 0}
        failures = []
        for _ in range(a.cases):
            verb, pool = rng.choice([("review", REVIEW_POOL), ("negotiate", NEGOTIATE_POOL)])
            argv = [verb] + [rng.choice(pool) for _ in range(rng.randint(1, 6))]
            stats["cases"] += 1
            out = rust_normalise(a.lope, stub, argv)
            before = oracle(parser, argv)
            if out == argv:
                stats["unchanged"] += 1
                continue
            if out is None:
                stats["rejected"] += 1
                if before is not None:
                    failures.append(("rejected an argv Python accepts", argv, before))
                continue
            stats["rewritten"] += 1
            after = oracle(parser, out)
            if before is not None:
                failures.append(("rewrote an argv Python accepts", argv, out))
            elif verb == "negotiate" and oracle(aliased, argv) != after:
                failures.append(("rewrite differs from a real --rounds alias", argv, out))
            elif verb == "review" and after is not None and not after.get("consensus"):
                failures.append(("rewrite lost --consensus", argv, out))
            elif verb == "review" and after is not None and after.get("file") in ("quorum",) and "quorum" not in out[1:]:
                failures.append(("file became the mode token", argv, out))
        print("normalise-fuzz: %s" % json.dumps(stats))
        for f in failures[:20]:
            print("FAIL %s: %s -> %s" % f)
        print("result: %s (%d failures)" % ("PASS" if not failures else "FAIL", len(failures)))
        return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
