#!/usr/bin/env python3
"""Dump Python Lope's argparse tree as canonical JSON (Spec 03 NFR14, S0 task 4a).

The parser is built inside ``lope.cli.main()``, so this script runs ``main()``
with ``ArgumentParser.parse_args`` patched to raise before any command executes.
Nothing in ``$LOPE_HOME`` is modified.

Usage: LOPE_HOME=~/.lope python3 dump_python.py > python.json
(run through tools/parity/dump-python, which scrubs the environment).
"""
import argparse
import json
import os
import sys

SCHEMA = 1


class _Captured(Exception):
    def __init__(self, parser):
        super().__init__("captured")
        self.parser = parser


def _kind(action):
    names = {
        argparse._StoreAction: "store",
        argparse._StoreTrueAction: "store_true",
        argparse._StoreFalseAction: "store_false",
        argparse._StoreConstAction: "store_const",
        argparse._AppendAction: "append",
        argparse._AppendConstAction: "append_const",
        argparse._CountAction: "count",
        argparse._HelpAction: "help",
        argparse._VersionAction: "version",
        argparse._SubParsersAction: "parsers",
    }
    extend = getattr(argparse, "_ExtendAction", None)
    if extend is not None and type(action) is extend:
        return "extend"
    for cls, name in names.items():
        if type(action) is cls:
            return name
    return "custom:" + type(action).__name__


def _value(v):
    """Canonical JSON form of a default / const / choice."""
    if v is argparse.SUPPRESS:
        return "==SUPPRESS=="
    if v is None or isinstance(v, (bool, int, float, str)):
        return v
    if isinstance(v, (list, tuple)):
        return [_value(x) for x in v]
    return "repr:" + repr(v)


def _nargs(action):
    n = action.nargs
    if n is None or isinstance(n, int):
        return n
    return {"?": "?", "*": "*", "+": "+", argparse.REMAINDER: "...", argparse.PARSER: "A..."}.get(n, str(n))


def _type(action):
    t = action.type
    if t is None:
        return None
    return getattr(t, "__name__", repr(t))


def _action(action):
    return {
        "dest": action.dest,
        "option_strings": list(action.option_strings),
        "kind": _kind(action),
        "nargs": _nargs(action),
        "const": _value(action.const),
        "default": _value(action.default),
        "type": _type(action),
        "choices": None if action.choices is None else [_value(c) for c in action.choices],
        "required": bool(action.required),
        "metavar": _value(action.metavar),
        "help": action.help if action.help is not argparse.SUPPRESS else "==SUPPRESS==",
    }


def _parser(p):
    out = {
        "prog": p.prog,
        "description": p.description,
        "allow_abbrev": getattr(p, "allow_abbrev", True),
        "prefix_chars": p.prefix_chars,
        "options": [],
        "positionals": [],
        "mutex_groups": [],
        "subcommands": None,
    }
    for a in p._actions:
        kind = _kind(a)
        if kind == "parsers":
            help_by_name = {c.dest: c.help for c in a._choices_actions}
            groups = []  # [(parser, [names...])] in insertion order
            for name, sub in a.choices.items():
                for g in groups:
                    if g[0] is sub:
                        g[1].append(name)
                        break
                else:
                    groups.append((sub, [name]))
            out["subcommands"] = {
                "dest": a.dest,
                "required": bool(a.required),
                "commands": [
                    {
                        "name": names[0],
                        "aliases": names[1:],
                        "help": help_by_name.get(names[0]),
                        "parser": _parser(sub),
                    }
                    for sub, names in groups
                ],
            }
        elif a.option_strings:
            out["options"].append(_action(a))
        else:
            out["positionals"].append(_action(a))
    for g in p._mutually_exclusive_groups:
        out["mutex_groups"].append({
            "required": bool(g.required),
            "members": [a.option_strings[0] if a.option_strings else a.dest for a in g._group_actions],
        })
    return out


def main():
    home = os.environ.get("LOPE_HOME") or os.path.expanduser("~/.lope")
    sys.path.insert(0, os.path.expanduser(home))
    os.environ.pop("LOPE_IMPLEMENTATION_DEPTH", None)

    import lope
    import lope.cli

    original = argparse.ArgumentParser.parse_args

    def capture(self, args=None, namespace=None):
        if self.prog == "lope":
            raise _Captured(self)
        return original(self, args, namespace)

    argparse.ArgumentParser.parse_args = capture
    try:
        lope.cli.main()
    except _Captured as c:
        root = c.parser
    else:
        sys.exit("dump_python: lope.cli.main() returned without calling parse_args")
    finally:
        argparse.ArgumentParser.parse_args = original

    doc = {
        "schema": SCHEMA,
        "source": "python-lope",
        "lope_version": lope.__version__,
        "python_version": "%d.%d.%d" % sys.version_info[:3],
        "parser": _parser(root),
    }
    json.dump(doc, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
