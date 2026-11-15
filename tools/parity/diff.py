#!/usr/bin/env python3
"""A1 parity diff: argparse capture (dump_python.py) vs clap projection
(lope-parity-dump). Projects the argparse tree into the parse-relevant schema
documented in crates/lope-compat/src/dump.rs and compares field by field.

Differences listed in allowlist.json (additive entries, with a reason) are
reported but do not fail. Exit 0 = parity, 1 = differences, 2 = bad input.

Usage: diff.py python.json rust.json [allowlist.json]
"""
import json
import sys

KIND = {
    "store": "value", "append": "append", "count": "count", "help": "help",
    "store_true": "flag", "store_false": "flag", "store_const": "flag", "append_const": "flag",
}
TYPE = {None: "str", "str": "str", "int": "int", "float": "float", "_positive_int": "positive_int"}
NARGS = {None: [1, 1], "?": [0, 1], "*": [0, None], "+": [1, None]}


def arity(nargs):
    if isinstance(nargs, int):
        return [nargs, nargs]
    return NARGS[nargs]


def project_option(a):
    kind = KIND[a["kind"]]
    takes = kind in ("value", "append")
    return {
        "kind": kind,
        "arity": arity(a["nargs"]) if takes else [0, 0],
        "type": (TYPE[a["type"]] if a["choices"] is None else "str") if takes else "none",
        "choices": a["choices"] if takes else None,
        "required": a["required"],
    }


def project_positional(a):
    return {"arity": arity(a["nargs"]), "type": TYPE[a["type"]] if a["choices"] is None else "str", "choices": a["choices"]}


def key(strings):
    return ",".join(sorted(strings))


def project(p, name, aliases):
    by_first = {a["option_strings"][0]: a for a in p["options"]}
    groups = sorted(
        ({"required": g["required"], "members": sorted(key(by_first[m]["option_strings"]) for m in g["members"])}
         for g in p["mutex_groups"]),
        key=lambda g: json.dumps(g["members"]),
    )
    sc = p["subcommands"]
    return {
        "name": name,
        "aliases": sorted(aliases),
        "allow_abbrev": p["allow_abbrev"],
        "subcommands_required": bool(sc and sc["required"]),
        "options": {key(a["option_strings"]): project_option(a) for a in p["options"]},
        "positionals": [project_positional(a) for a in p["positionals"]],
        "mutex_groups": groups,
        "subcommands": {c["name"]: project(c["parser"], c["name"], c["aliases"]) for c in (sc["commands"] if sc else [])},
    }


def compare(py, rs, path, out):
    for field in ("aliases", "allow_abbrev", "subcommands_required", "positionals", "mutex_groups"):
        if py[field] != rs[field]:
            out.append((path, field, py[field], rs[field]))
    for k in sorted(set(py["options"]) | set(rs["options"])):
        a, b = py["options"].get(k), rs["options"].get(k)
        if a is None or b is None:
            out.append((path, "option " + k, a, b))
            continue
        for f in sorted(set(a) | set(b)):
            if a.get(f) != b.get(f):
                out.append((path, "option %s .%s" % (k, f), a.get(f), b.get(f)))
    for k in sorted(set(py["subcommands"]) | set(rs["subcommands"])):
        a, b = py["subcommands"].get(k), rs["subcommands"].get(k)
        sub = (path + " " + k).strip()
        if a is None or b is None:
            out.append((sub, "subcommand", a is not None, b is not None))
        else:
            compare(a, b, sub, out)


def main():
    if len(sys.argv) not in (3, 4):
        print(__doc__, file=sys.stderr)
        return 2
    py_doc = json.load(open(sys.argv[1]))
    rs_doc = json.load(open(sys.argv[2]))
    allow = json.load(open(sys.argv[3]))["entries"] if len(sys.argv) == 4 else []
    if py_doc.get("schema") != 1 or rs_doc.get("schema") != 1:
        print("diff.py: unsupported schema", file=sys.stderr)
        return 2
    py = project(py_doc["parser"], "lope", [])
    rs = rs_doc["root"]
    diffs = []
    compare(py, rs, "lope", diffs)
    allowed = {(e["path"], e["field"]) for e in allow}
    failing = [d for d in diffs if (d[0], d[1]) not in allowed]
    print("python-lope %s (argparse) vs lope v2 clap grammar (captured from %s)"
          % (py_doc["lope_version"], rs_doc.get("grammar_lope_version")))

    def count(p):
        return 1 + sum(count(c) for c in p["subcommands"].values())

    n_opts = []

    def walk(p):
        n_opts.append(len(p["options"]) + len(p["positionals"]))
        for c in p["subcommands"].values():
            walk(c)

    walk(py)
    print("compared %d commands, %d arguments" % (count(py), sum(n_opts)))
    for d in diffs:
        tag = "ALLOWED" if d not in failing else "DIFF"
        print("%-7s %s :: %s\n        python=%s\n        rust  =%s" % (tag, d[0], d[1], json.dumps(d[2]), json.dumps(d[3])))
    for e in allow:
        if (e["path"], e["field"]) not in {(d[0], d[1]) for d in diffs}:
            print("STALE   allow-list entry no longer needed: %s :: %s" % (e["path"], e["field"]))
    print("result: %s (%d differences, %d allow-listed)" % ("PASS" if not failing else "FAIL", len(diffs), len(diffs) - len(failing)))
    return 0 if not failing else 1


if __name__ == "__main__":
    sys.exit(main())
