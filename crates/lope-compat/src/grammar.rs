//! Python Lope's CLI grammar as static data, generated at build time from the
//! argparse capture (see build.rs). It drives argv normalisation, the clap
//! parser and the parity dump.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Store,
    StoreTrue,
    StoreFalse,
    StoreConst,
    Append,
    AppendConst,
    Count,
    Help,
}

impl Kind {
    /// Number of values the action can take when no explicit `=value` is given.
    pub fn takes_values(self) -> bool {
        matches!(self, Kind::Store | Kind::Append)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nargs {
    One,
    Optional,
    ZeroOrMore,
    OneOrMore,
    Exactly(u32),
}

impl Nargs {
    /// (min, max) values; `None` = unbounded.
    pub fn range(self) -> (usize, Option<usize>) {
        match self {
            Nargs::One => (1, Some(1)),
            Nargs::Optional => (0, Some(1)),
            Nargs::ZeroOrMore => (0, None),
            Nargs::OneOrMore => (1, None),
            Nargs::Exactly(n) => (n as usize, Some(n as usize)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Str,
    Int,
    Float,
    /// `cli.py:_positive_int`: `int(value)` that must be > 0.
    PositiveInt,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OptSpec {
    pub dest: &'static str,
    pub option_strings: &'static [&'static str],
    pub kind: Kind,
    pub nargs: Nargs,
    pub ty: ValueType,
    pub choices: Option<&'static [&'static str]>,
    pub required: bool,
    /// The argparse default, as JSON text (`null`, `3`, `"auto"`, …).
    pub default_json: &'static str,
    pub help: Option<&'static str>,
    pub metavar: Option<&'static str>,
}

impl OptSpec {
    /// clap id: the first long option string (with dashes), else the first
    /// short one. Option ids start with `-`, so they never collide with
    /// positional ids (the argparse dest).
    pub fn id(&self) -> &'static str {
        self.option_strings
            .iter()
            .find(|o| o.starts_with("--"))
            .copied()
            .unwrap_or(self.option_strings[0])
    }

    pub fn arity(&self) -> (usize, Option<usize>) {
        if self.kind.takes_values() {
            self.nargs.range()
        } else {
            (0, Some(0))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct PosSpec {
    pub dest: &'static str,
    pub nargs: Nargs,
    pub ty: ValueType,
    pub choices: Option<&'static [&'static str]>,
    pub default_json: &'static str,
    pub help: Option<&'static str>,
    pub metavar: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MutexSpec {
    pub required: bool,
    /// First option string of each member.
    pub members: &'static [&'static str],
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub help: Option<&'static str>,
    pub description: Option<&'static str>,
    pub allow_abbrev: bool,
    pub subcommands_required: bool,
    pub options: &'static [OptSpec],
    pub positionals: &'static [PosSpec],
    pub mutex_groups: &'static [MutexSpec],
    pub subcommands: &'static [CommandSpec],
}

impl CommandSpec {
    /// Subcommand by name or alias.
    pub fn subcommand(&self, name: &str) -> Option<&'static CommandSpec> {
        self.subcommands
            .iter()
            .find(|c| c.name == name || c.aliases.contains(&name))
    }

    pub fn option_by_string(&self, s: &str) -> Option<&'static OptSpec> {
        self.options.iter().find(|o| o.option_strings.contains(&s))
    }

    pub fn option_strings(&self) -> impl Iterator<Item = (&'static str, &'static OptSpec)> {
        self.options
            .iter()
            .flat_map(|o| o.option_strings.iter().map(move |s| (*s, o)))
    }
}

pub const SUPPRESS: &str = "==SUPPRESS==";

include!(concat!(env!("OUT_DIR"), "/grammar_gen.rs"));

/// Top-level verb spec.
pub fn verb(name: &str) -> Option<&'static CommandSpec> {
    ROOT.subcommand(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_has_all_23_verbs() {
        assert_eq!(ROOT.subcommands.len(), 23);
        assert_eq!(
            GRAMMAR_LOPE_VERSION,
            lope_core::COMPAT_VERSION,
            "grammar capture and COMPAT_VERSION disagree"
        );
    }

    #[test]
    fn glimmer_checked_review_flags_exist() {
        let review = verb("review").unwrap();
        for f in ["--consensus", "--format", "--focus", "--brain-context"] {
            assert!(review.option_by_string(f).is_some(), "{f}");
        }
        // Two separate store_true actions sharing dest `consensus`, not aliases.
        let structured = review.option_by_string("--structured").unwrap();
        assert_eq!(
            (structured.id(), structured.dest),
            ("--structured", "consensus")
        );
        assert!(verb("negotiate")
            .unwrap()
            .option_by_string("--max-rounds")
            .is_some());
        assert!(verb("negotiate")
            .unwrap()
            .option_by_string("--rounds")
            .is_none());
    }

    #[test]
    fn flow_validate_has_lint_alias() {
        let flow = verb("flow").unwrap();
        assert_eq!(flow.subcommand("lint").unwrap().name, "validate");
    }
}
