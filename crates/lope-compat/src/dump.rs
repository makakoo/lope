//! Parse-relevant projection of the clap tree, for the A1 parity diff.
//!
//! Reads only what clap itself reports after `Command::build()`, never the
//! argparse capture, so the diff genuinely checks the clap grammar. The same
//! projection is computed from the argparse dump by tools/parity/diff.py.
//!
//! Projection schema (shared with diff.py):
//! `{name, aliases, allow_abbrev, subcommands_required,
//!   options: {"<sorted option strings, comma-joined>": {kind, arity, type, choices, required}},
//!   positionals: [{arity, type, choices}], mutex_groups: [{required, members}],
//!   subcommands: {name: projection}}`

use crate::grammar;
use crate::parser;
use clap::builder::{ValueParser, ValueRange};
use clap::error::ErrorKind;
use clap::{Arg, ArgAction, Command};
use serde_json::{json, Map, Value};

pub fn projection() -> Value {
    let mut root = Command::new("lope")
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .infer_long_args(grammar::ROOT.allow_abbrev);
    for s in grammar::ROOT.subcommands {
        root = root.subcommand(parser::build(s));
    }
    root.build();
    json!({
        "schema": 1,
        "source": "rust-clap",
        "grammar_lope_version": grammar::GRAMMAR_LOPE_VERSION,
        "root": command(&root),
    })
}

fn command(cmd: &Command) -> Value {
    let mut aliases: Vec<&str> = cmd.get_all_aliases().collect();
    aliases.sort_unstable();
    let mut options = Map::new();
    let mut positionals: Vec<(usize, Value)> = Vec::new();
    for a in cmd.get_arguments() {
        if a.is_positional() {
            positionals.push((a.get_index().unwrap_or(usize::MAX), positional(a)));
        } else {
            options.insert(option_key(a), option(a));
        }
    }
    positionals.sort_by_key(|(i, _)| *i);
    let mut groups: Vec<Value> = cmd
        .get_groups()
        .map(|g| {
            let mut members: Vec<String> = g
                .get_args()
                .map(|id| {
                    let arg = cmd
                        .get_arguments()
                        .find(|a| a.get_id() == id)
                        .expect("group member exists");
                    option_key(arg)
                })
                .collect();
            members.sort();
            json!({ "required": g.is_required_set(), "members": members })
        })
        .collect();
    groups.sort_by_key(|g| g["members"].to_string());
    let subcommands: Map<String, Value> = cmd
        .get_subcommands()
        .map(|s| (s.get_name().to_string(), command(s)))
        .collect();
    json!({
        "name": cmd.get_name(),
        "aliases": aliases,
        "allow_abbrev": accepts_abbreviations(cmd),
        "subcommands_required": cmd.is_subcommand_required_set(),
        "options": options,
        "positionals": positionals.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "mutex_groups": groups,
        "subcommands": subcommands,
    })
}

/// clap does not expose `infer_long_args`, so probe it: parse a unique
/// one-character-short prefix of a long option and see whether clap resolves
/// it (any outcome other than "unknown argument").
fn accepts_abbreviations(cmd: &Command) -> bool {
    let longs: Vec<String> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional())
        .flat_map(|a| {
            a.get_long()
                .into_iter()
                .chain(a.get_all_aliases().unwrap_or_default())
                .map(|l| format!("--{l}"))
        })
        .collect();
    let probe = longs.iter().map(|l| &l[..l.len() - 1]).find(|p| {
        p.len() > 2
            && longs.iter().filter(|l| l.starts_with(p)).count() == 1
            && !longs.iter().any(|l| l == p)
    });
    let Some(probe) = probe else { return false };
    match cmd.clone().try_get_matches_from([cmd.get_name(), probe]) {
        Ok(_) => true,
        Err(e) => e.kind() != ErrorKind::UnknownArgument,
    }
}

fn option_key(a: &Arg) -> String {
    let mut strings: Vec<String> = Vec::new();
    if let Some(l) = a.get_long() {
        strings.push(format!("--{l}"));
    }
    if let Some(s) = a.get_short() {
        strings.push(format!("-{s}"));
    }
    for l in a.get_all_aliases().unwrap_or_default() {
        strings.push(format!("--{l}"));
    }
    for s in a.get_all_short_aliases().unwrap_or_default() {
        strings.push(format!("-{s}"));
    }
    strings.sort();
    strings.join(",")
}

fn arity(a: &Arg) -> Value {
    let r = a.get_num_args().unwrap_or_else(|| ValueRange::new(0..=0));
    let max = if r.max_values() == usize::MAX {
        Value::Null
    } else {
        json!(r.max_values())
    };
    json!([r.min_values(), max])
}

fn kind(a: &Arg) -> &'static str {
    match a.get_action() {
        ArgAction::Set => "value",
        ArgAction::Append => "append",
        ArgAction::SetTrue | ArgAction::SetFalse => "flag",
        ArgAction::Count => "count",
        ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong => "help",
        ArgAction::Version => "version",
        _ => "unknown",
    }
}

fn value_type(a: &Arg) -> &'static str {
    if !a.get_action().takes_values() {
        return "none";
    }
    if !a.get_possible_values().is_empty() {
        return "str";
    }
    // clap's AnyValueId is not nameable outside clap, so compare against the
    // ids of reference parsers.
    let t = a.get_value_parser().type_id();
    let id = |p: ValueParser| p.type_id();
    if t == id(clap::value_parser!(i64).into()) {
        "int"
    } else if t == id(clap::value_parser!(u64).into()) {
        // Only cli.py:_positive_int maps to u64; the range itself is checked
        // by parser::tests.
        "positive_int"
    } else if t == id(clap::value_parser!(f64).into()) {
        "float"
    } else if t == id(ValueParser::os_string()) || t == id(ValueParser::string()) {
        "str"
    } else {
        "unknown"
    }
}

fn choices(a: &Arg) -> Value {
    let pv = a.get_possible_values();
    if pv.is_empty() || !a.get_action().takes_values() {
        Value::Null
    } else {
        json!(pv
            .iter()
            .map(|p| p.get_name().to_string())
            .collect::<Vec<_>>())
    }
}

fn option(a: &Arg) -> Value {
    let arity = if a.get_action().takes_values() {
        arity(a)
    } else {
        json!([0, 0])
    };
    json!({
        "kind": kind(a),
        "arity": arity,
        "type": value_type(a),
        "choices": choices(a),
        "required": a.is_required_set(),
    })
}

fn positional(a: &Arg) -> Value {
    let r = a.get_num_args().unwrap_or_else(|| ValueRange::new(1..=1));
    let min = if a.is_required_set() {
        r.min_values()
    } else {
        0
    };
    let max = if r.max_values() == usize::MAX {
        Value::Null
    } else {
        json!(r.max_values())
    };
    json!({ "arity": [min, max], "type": value_type(a), "choices": choices(a) })
}
