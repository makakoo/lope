//! Argv normalisation applied before routing (FR2, FR23, DEC-11).
//!
//! - top-level `--version` / `-V`            → `version`
//! - `review … --consensus quorum`          → `review … --consensus`
//!   (`quorum` is consumed only when it immediately follows the exact token
//!   `--consensus` *and* the `file` positional is present elsewhere;
//!   `unanimous` / `any` in that position exit 2 until OQ-1 is decided)
//! - `negotiate … --rounds N | --rounds=N`  → `--max-rounds N | --max-rounds=N`
//!
//! Tokens are classified the way argparse does (`_parse_optional`), so option
//! values, `--`, abbreviations and negative numbers are never rewritten by
//! mistake.

use crate::grammar::{self, CommandSpec, OptSpec};
use std::ffi::{OsStr, OsString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    pub prog: String,
    pub message: String,
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: error: {}", self.prog, self.message)
    }
}

/// What normalisation changed, for `LOPE_ROUTER_TRACE` and tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Rewrites {
    pub version_flag: bool,
    pub consensus_quorum: usize,
    pub negotiate_rounds: usize,
}

impl Rewrites {
    pub fn any(&self) -> bool {
        self.version_flag || self.consensus_quorum > 0 || self.negotiate_rounds > 0
    }
}

pub fn normalize(mut args: Vec<OsString>) -> Result<(Vec<OsString>, Rewrites), UsageError> {
    let mut rw = Rewrites::default();
    if let Some(first) = args.first() {
        if first == "--version" || first == "-V" {
            args[0] = OsString::from("version");
            rw.version_flag = true;
        }
    }
    let verb = args.first().and_then(|a| a.to_str()).unwrap_or("");
    match verb {
        "review" => {
            let spec = grammar::verb("review").expect("review in grammar");
            rw.consensus_quorum = consensus_quorum(&mut args, spec)?;
        }
        "negotiate" => {
            let spec = grammar::verb("negotiate").expect("negotiate in grammar");
            rw.negotiate_rounds = negotiate_rounds(&mut args, spec);
        }
        _ => {}
    }
    Ok((args, rw))
}

/// An extra option string treated as an existing option (`--rounds`).
type Alias = Option<(&'static str, &'static OptSpec)>;

/// argparse's view of one token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// `--`: everything after it is positional.
    DoubleDash,
    /// Positional-looking ('A').
    Arg,
    /// Option-looking ('O'): the resolved option (if known) and whether the
    /// value was attached (`--x=v`, `-ov`).
    Opt(Option<&'static OptSpec>, bool),
}

/// Port of CPython 3.9 `ArgumentParser._parse_optional` (+ `_get_option_tuples`)
/// for one parser. `alias` lets callers treat an extra option string as an
/// existing option (used for `negotiate --rounds`).
fn classify(spec: &'static CommandSpec, raw: &OsStr, alias: Alias) -> Token {
    let lossy = raw.to_string_lossy();
    let arg: &str = &lossy;
    if arg == "--" {
        return Token::DoubleDash;
    }
    if arg.is_empty() || !arg.starts_with('-') {
        return Token::Arg;
    }
    let lookup = |s: &str| -> Option<&'static OptSpec> {
        match alias {
            Some((a, o)) if a == s => Some(o),
            _ => spec.option_by_string(s),
        }
    };
    if let Some(o) = lookup(arg) {
        return Token::Opt(Some(o), false);
    }
    if arg.len() == 1 {
        return Token::Arg;
    }
    if let Some((name, _)) = arg.split_once('=') {
        if let Some(o) = lookup(name) {
            return Token::Opt(Some(o), true);
        }
    }
    if spec.allow_abbrev || !arg.starts_with("--") {
        let mut matches: Vec<(&'static OptSpec, bool)> = Vec::new();
        let all = spec.option_strings().chain(alias);
        if arg.starts_with("--") {
            let prefix = arg.split_once('=').map(|(p, _)| p).unwrap_or(arg);
            let explicit = arg.contains('=');
            for (s, o) in all {
                if s.starts_with(prefix) {
                    matches.push((o, explicit));
                }
            }
        } else {
            let short_prefix = &arg[..arg.char_indices().nth(2).map(|(i, _)| i).unwrap_or(arg.len())];
            for (s, o) in all {
                if s == short_prefix {
                    matches.push((o, arg.len() > short_prefix.len()));
                } else if s.starts_with(arg) {
                    matches.push((o, false));
                }
            }
        }
        match matches.len() {
            1 => return Token::Opt(Some(matches[0].0), matches[0].1),
            n if n > 1 => return Token::Opt(None, false), // ambiguous: argparse errors
            _ => {}
        }
    }
    if is_negative_number(arg) && !spec.option_strings().any(|(s, _)| is_negative_number(s)) {
        return Token::Arg;
    }
    if arg.contains(' ') {
        return Token::Arg;
    }
    Token::Opt(None, false)
}

/// argparse `_negative_number_matcher`: `^-\d+$|^-\d*\.\d+$`.
fn is_negative_number(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('-') else {
        return false;
    };
    if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    match rest.split_once('.') {
        Some((int, frac)) => {
            int.bytes().all(|b| b.is_ascii_digit()) && !frac.is_empty() && frac.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

/// Per-token role after argparse-style value consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Verb,
    Option(Option<&'static OptSpec>),
    Value,
    Positional,
    AfterDoubleDash,
    DoubleDash,
}

fn roles(args: &[OsString], spec: &'static CommandSpec, alias: Alias) -> Vec<Role> {
    let mut out = vec![Role::Verb];
    let mut i = 1;
    let mut after_dd = false;
    while i < args.len() {
        if after_dd {
            out.push(Role::AfterDoubleDash);
            i += 1;
            continue;
        }
        match classify(spec, &args[i], alias) {
            Token::DoubleDash => {
                after_dd = true;
                out.push(Role::DoubleDash);
            }
            Token::Arg => out.push(Role::Positional),
            Token::Opt(o, explicit) => {
                out.push(Role::Option(o));
                let max = match o {
                    Some(o) if !explicit => o.arity().1.unwrap_or(usize::MAX),
                    _ => 0,
                };
                let mut taken = 0;
                while taken < max && i + 1 < args.len() && classify(spec, &args[i + 1], alias) == Token::Arg {
                    i += 1;
                    taken += 1;
                    out.push(Role::Value);
                }
            }
        }
        i += 1;
    }
    out
}

fn consensus_quorum(args: &mut Vec<OsString>, spec: &'static CommandSpec) -> Result<usize, UsageError> {
    let mut removed = 0;
    loop {
        let r = roles(args, spec, None);
        let positionals = r.iter().filter(|x| **x == Role::Positional).count();
        let hit = (1..args.len().saturating_sub(1)).find(|&i| {
            args[i] == "--consensus"
                && matches!(r[i], Role::Option(_))
                && r[i + 1] == Role::Positional
                && positionals >= 2
                && matches!(args[i + 1].to_str(), Some("quorum" | "unanimous" | "any"))
        });
        let Some(i) = hit else { return Ok(removed) };
        let mode = args[i + 1].to_string_lossy().into_owned();
        if mode != "quorum" {
            return Err(UsageError {
                prog: "lope review".into(),
                message: format!(
                    "--consensus {mode} is not supported; use --consensus or --consensus quorum \
                     (the meaning of '{mode}' is undecided, see super-lope OQ-1)"
                ),
            });
        }
        args.remove(i + 1);
        removed += 1;
    }
}

fn negotiate_rounds(args: &mut [OsString], spec: &'static CommandSpec) -> usize {
    let max_rounds = spec.option_by_string("--max-rounds").expect("--max-rounds in grammar");
    let r = roles(args, spec, Some(("--rounds", max_rounds)));
    let mut n = 0;
    for (i, role) in r.iter().enumerate() {
        if !matches!(role, Role::Option(_)) {
            continue;
        }
        let Some(tok) = args[i].to_str() else {
            continue;
        };
        let replaced = if tok == "--rounds" {
            Some("--max-rounds".to_string())
        } else {
            tok.strip_prefix("--rounds=").map(|v| format!("--max-rounds={v}"))
        };
        if let Some(new) = replaced {
            args[i] = OsString::from(new);
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn norm(args: &[&str]) -> Result<Vec<String>, UsageError> {
        normalize(v(args)).map(|(a, _)| a.into_iter().map(|x| x.into_string().unwrap()).collect())
    }

    #[test]
    fn version_flags() {
        assert_eq!(norm(&["--version"]).unwrap(), ["version"]);
        assert_eq!(norm(&["-V"]).unwrap(), ["version"]);
        assert_eq!(norm(&["review", "--version"]).unwrap(), ["review", "--version"]);
        assert_eq!(norm(&["-h", "--version"]).unwrap(), ["-h", "--version"]);
    }

    #[test]
    fn glimmer_council_runner_argv() {
        // council-runner.ts:79
        assert_eq!(
            norm(&["review", "notes.md", "--consensus", "quorum", "--format", "markdown"]).unwrap(),
            ["review", "notes.md", "--consensus", "--format", "markdown"]
        );
        assert_eq!(
            norm(&["review", "--consensus", "quorum", "notes.md"]).unwrap(),
            ["review", "--consensus", "notes.md"]
        );
    }

    #[test]
    fn quorum_kept_when_it_is_the_file() {
        assert_eq!(norm(&["review", "--consensus", "quorum"]).unwrap(), ["review", "--consensus", "quorum"]);
        // `--focus` consumes `x.md`, so `quorum` is the only positional.
        assert_eq!(
            norm(&["review", "--focus", "x.md", "--consensus", "quorum"]).unwrap(),
            ["review", "--focus", "x.md", "--consensus", "quorum"]
        );
    }

    #[test]
    fn only_the_exact_consensus_token_is_normalised() {
        for flag in ["--structured", "--consen", "--consensus=quorum"] {
            let args = ["review", "f.md", flag, "quorum"];
            let out = norm(&args).unwrap();
            assert_eq!(out, args, "{flag}");
        }
        // a value of another option is never touched
        let args = ["review", "f.md", "--focus", "quorum"];
        assert_eq!(norm(&args).unwrap(), args);
        // after `--` nothing is rewritten
        let args = ["review", "--consensus", "--", "quorum", "f.md"];
        assert_eq!(norm(&args).unwrap(), args);
    }

    #[test]
    fn unanimous_and_any_are_rejected() {
        for mode in ["unanimous", "any"] {
            let err = norm(&["review", "f.md", "--consensus", mode]).unwrap_err();
            assert_eq!(err.prog, "lope review");
            assert!(err.message.contains(mode));
        }
        // ... but only in the quorum position
        assert_eq!(norm(&["review", "--consensus", "any"]).unwrap(), ["review", "--consensus", "any"]);
    }

    #[test]
    fn negotiate_rounds_alias() {
        // plan-build.ts:135
        assert_eq!(
            norm(&["negotiate", "goal", "--domain", "engineering", "--rounds", "3"]).unwrap(),
            ["negotiate", "goal", "--domain", "engineering", "--max-rounds", "3"]
        );
        assert_eq!(norm(&["negotiate", "g", "--rounds=2"]).unwrap(), ["negotiate", "g", "--max-rounds=2"]);
        // argparse never takes an option-looking token as a value, so this is
        // still the alias (Python errors on the missing --context value either way)
        let args = ["negotiate", "g", "--context", "--rounds"];
        assert_eq!(norm(&args).unwrap(), ["negotiate", "g", "--context", "--max-rounds"]);
        // inline value of another option, or after `--`: untouched
        let args = ["negotiate", "g", "--context=--rounds"];
        assert_eq!(norm(&args).unwrap(), args);
        let args = ["negotiate", "--", "--rounds", "3"];
        assert_eq!(norm(&args).unwrap(), args);
        // other verbs: untouched
        let args = ["execute", "s.md", "--rounds", "3"];
        assert_eq!(norm(&args).unwrap(), args);
    }

    #[test]
    fn negative_numbers_are_values() {
        assert!(is_negative_number("-1"));
        assert!(is_negative_number("-.5"));
        assert!(is_negative_number("-1.5"));
        assert!(!is_negative_number("-"));
        assert!(!is_negative_number("-x"));
        assert!(!is_negative_number("-1."));
    }
}
