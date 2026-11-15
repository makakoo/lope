//! clap grammar built from the argparse capture (S0 task 5). Used to decide
//! coverage and, from S1 on, to parse arguments for native verbs. Any clap
//! error (including `--help`) means "not covered": the invocation is then
//! delegated and Python produces its exact output.

use crate::grammar::{self, CommandSpec, Kind, Nargs, OptSpec, PosSpec, ValueType, SUPPRESS};
use clap::builder::{PossibleValuesParser, ValueParser};
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgGroup, Command};
use std::ffi::OsString;

/// clap ids must be `'static` without the `string` feature.
const MUTEX_IDS: [&str; 8] = ["mutex-0", "mutex-1", "mutex-2", "mutex-3", "mutex-4", "mutex-5", "mutex-6", "mutex-7"];

/// Build the clap command for one argparse parser (recursively).
pub fn build(spec: &'static CommandSpec) -> Command {
    let mut cmd = Command::new(spec.name)
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .args_override_self(true)
        .infer_long_args(spec.allow_abbrev)
        .subcommand_required(spec.subcommands_required);
    if !spec.aliases.is_empty() {
        cmd = cmd.visible_aliases(spec.aliases.iter().copied());
    }
    if let Some(h) = spec.help {
        cmd = cmd.about(h);
    }
    if let Some(d) = spec.description {
        cmd = cmd.long_about(d);
    }
    for o in spec.options {
        if let Some(arg) = option(o) {
            cmd = cmd.arg(arg);
        }
    }
    for (i, p) in spec.positionals.iter().enumerate() {
        cmd = cmd.arg(positional(p, i + 1));
    }
    for (i, g) in spec.mutex_groups.iter().enumerate() {
        let ids = g.members.iter().map(|m| spec.option_by_string(m).map(OptSpec::id).unwrap_or(m));
        let id = *MUTEX_IDS.get(i).expect("more mutex groups than MUTEX_IDS; extend it");
        cmd = cmd.group(ArgGroup::new(id).args(ids).multiple(false).required(g.required));
    }
    for s in spec.subcommands {
        cmd = cmd.subcommand(build(s));
    }
    cmd
}

fn value_parser(ty: ValueType, choices: Option<&'static [&'static str]>) -> ValueParser {
    if let Some(c) = choices {
        return PossibleValuesParser::new(c.iter().copied()).into();
    }
    match ty {
        ValueType::Str => ValueParser::os_string(),
        ValueType::Int => clap::value_parser!(i64).into(),
        ValueType::Float => clap::value_parser!(f64).into(),
        ValueType::PositiveInt => clap::value_parser!(u64).range(1..).into(),
    }
}

fn apply_nargs(arg: Arg, nargs: Nargs) -> Arg {
    match nargs {
        Nargs::One => arg.num_args(1),
        Nargs::Optional => arg.num_args(0..=1),
        Nargs::ZeroOrMore => arg.num_args(0..),
        Nargs::OneOrMore => arg.num_args(1..),
        Nargs::Exactly(n) => arg.num_args(n as usize),
    }
}

fn option(o: &'static OptSpec) -> Option<Arg> {
    if o.kind == Kind::Help {
        // clap's automatic -h/--help; parity is checked by the dump.
        return None;
    }
    let mut arg = Arg::new(o.id()).required(o.required);
    let mut longs = o.option_strings.iter().filter_map(|s| s.strip_prefix("--"));
    let mut shorts =
        o.option_strings.iter().filter(|s| !s.starts_with("--")).map(|s| s.chars().nth(1).expect("short option"));
    if let Some(l) = longs.next() {
        arg = arg.long(l);
    }
    if let Some(s) = shorts.next() {
        arg = arg.short(s);
    }
    arg = arg.visible_aliases(longs.collect::<Vec<_>>()).visible_short_aliases(shorts.collect::<Vec<_>>());
    arg = match o.kind {
        Kind::Store | Kind::Append => {
            let action = if o.kind == Kind::Store { ArgAction::Set } else { ArgAction::Append };
            let arg = arg.action(action).value_parser(value_parser(o.ty, o.choices)).allow_negative_numbers(true);
            let arg = arg.value_name(o.metavar.unwrap_or(o.dest));
            apply_nargs(arg, o.nargs)
        }
        Kind::StoreTrue | Kind::StoreConst | Kind::AppendConst => arg.action(ArgAction::SetTrue),
        Kind::StoreFalse => arg.action(ArgAction::SetFalse),
        Kind::Count => arg.action(ArgAction::Count),
        Kind::Help => unreachable!(),
    };
    Some(match o.help {
        Some(SUPPRESS) => arg.hide(true),
        Some(h) => arg.help(h),
        None => arg,
    })
}

fn positional(p: &'static PosSpec, index: usize) -> Arg {
    let (min, _) = p.nargs.range();
    let arg = Arg::new(p.dest)
        .index(index)
        .action(ArgAction::Set)
        .required(min > 0)
        .value_parser(value_parser(p.ty, p.choices))
        .allow_negative_numbers(true)
        .value_name(p.metavar.unwrap_or(p.dest));
    let arg = apply_nargs(arg, p.nargs);
    match p.help {
        Some(SUPPRESS) => arg.hide(true),
        Some(h) => arg.help(h),
        None => arg,
    }
}

/// A successfully parsed invocation: the canonical command path and the ids
/// of arguments given on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub path: Vec<&'static str>,
    pub present: Vec<String>,
    pub matches: clap::ArgMatches,
}

/// Parse normalised argv (without the program name). Builds only the subtree
/// of the requested verb, to keep router overhead low.
pub fn parse(args: &[OsString]) -> Result<Invocation, clap::Error> {
    let verb = args.first().and_then(|a| a.to_str()).and_then(grammar::verb);
    let root = Command::new("lope")
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .infer_long_args(grammar::ROOT.allow_abbrev);
    let root = match verb {
        Some(v) => root.subcommand(build(v)),
        None => root,
    };
    let argv = std::iter::once(OsString::from("lope")).chain(args.iter().cloned());
    let m = root.try_get_matches_from(argv)?;
    let mut path = Vec::new();
    let mut spec = &grammar::ROOT;
    let mut cur = &m;
    while let Some((name, sub)) = cur.subcommand() {
        spec = spec.subcommand(name).expect("clap subcommand from grammar");
        path.push(spec.name);
        cur = sub;
    }
    let present = cur
        .ids()
        .filter(|id| cur.value_source(id.as_str()) == Some(ValueSource::CommandLine))
        .filter(|id| !id.as_str().starts_with("mutex-"))
        .map(|id| id.as_str().to_string())
        .collect();
    Ok(Invocation { path, present, matches: cur.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Invocation, clap::Error> {
        parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn full_tree_builds_and_passes_clap_debug_asserts() {
        build(&grammar::ROOT).debug_assert();
    }

    #[test]
    fn parses_glimmer_argv_after_normalisation() {
        let inv = p(&["review", "notes.md", "--consensus", "--format", "markdown"]).unwrap();
        assert_eq!(inv.path, ["review"]);
        let mut present = inv.present.clone();
        present.sort();
        assert_eq!(present, ["--consensus", "--format", "file"]);
        let inv = p(&["negotiate", "g", "--domain", "engineering", "--max-rounds", "3"]).unwrap();
        assert_eq!(inv.matches.get_one::<i64>("--max-rounds"), Some(&3));
    }

    #[test]
    fn aliases_abbreviations_and_nesting() {
        assert_eq!(p(&["flow", "lint", "x.dot"]).unwrap().path, ["flow", "validate"]);
        let inv = p(&["review", "f.md", "--structured", "-o", "r.md"]).unwrap();
        let mut present = inv.present;
        present.sort();
        assert_eq!(present, ["--output", "--structured", "file"]);
        assert!(p(&["review", "f.md", "--form", "json"]).unwrap().present.contains(&"--format".to_string()));
        assert_eq!(p(&["jobs", "list"]).unwrap().path, ["jobs", "list"]);
    }

    #[test]
    fn errors_mean_delegate() {
        assert!(p(&["review"]).is_err()); // missing file
        assert!(p(&["review", "f.md", "--format", "yaml"]).is_err()); // bad choice
        assert!(p(&["version", "--help"]).is_err()); // help is Python's
        assert!(p(&["bogus"]).is_err());
        assert!(p(&["review", "f.md", "--run-timeout", "5", "--allow-unbounded-run"]).is_err());
        // mutex
    }
}
