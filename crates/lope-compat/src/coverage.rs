//! The native coverage table (FR5, Spec 01 §5): the single place that says
//! which invocations run in Rust. Everything else is delegated to Python Lope.
//! It grows sprint by sprint, each entry backed by parity tests.

/// One natively implemented command path and the options it supports.
#[derive(Debug)]
pub struct Coverage {
    /// Verb and (for nested commands) subcommand, by canonical name.
    pub path: &'static [&'static str],
    /// clap ids (option strings / positional dests) that may be present.
    /// An invocation using any other argument is delegated.
    pub args: &'static [&'static str],
    /// Sprint that added the entry.
    pub since: &'static str,
}

pub static COVERAGE: &[Coverage] = &[
    Coverage { path: &["version"], args: &[], since: "S0" },
    Coverage { path: &["status"], args: &[], since: "S1" },
    Coverage { path: &["jobs"], args: &[], since: "S1" },
    Coverage { path: &["jobs", "list"], args: &["--json"], since: "S1" },
    Coverage { path: &["jobs", "reap"], args: &["--dry-run", "--stale", "--json"], since: "S1" },
    Coverage { path: &["jobs", "prune"], args: &["--stale", "--dry-run", "--json"], since: "S1" },
    Coverage { path: &["jobs", "kill"], args: &["run_id", "--json"], since: "S1" },
    // S4 (first fan-out verb). --synth/--anonymous/--brain-* stay delegated.
    Coverage {
        path: &["ask"],
        args: &[
            "question",
            "--json",
            "--context",
            "--validators",
            "--primary",
            "--timeout",
            "--respect-provider-timeout",
            "--run-timeout",
            "--allow-unbounded-run",
            "--max-calls",
            "--max-chunks",
            "--request-policy",
            "--parallel",
            "--sequential",
        ],
        since: "S4",
    },
    Coverage {
        path: &["pipe"],
        args: &[
            "--require-all",
            "--json",
            "--validators",
            "--primary",
            "--timeout",
            "--respect-provider-timeout",
            "--run-timeout",
            "--allow-unbounded-run",
            "--max-calls",
            "--max-chunks",
            "--request-policy",
            "--parallel",
            "--sequential",
        ],
        since: "S4",
    },
    Coverage {
        path: &["vote"],
        args: &[
            "prompt",
            "--options",
            "--json",
            "--validators",
            "--primary",
            "--timeout",
            "--respect-provider-timeout",
            "--run-timeout",
            "--allow-unbounded-run",
            "--max-calls",
            "--max-chunks",
            "--request-policy",
            "--parallel",
            "--sequential",
        ],
        since: "S4",
    },
];

pub fn verb_has_coverage(verb: &str) -> bool {
    COVERAGE.iter().any(|c| c.path.first() == Some(&verb))
}

pub fn is_covered(path: &[&str], present: &[&str]) -> bool {
    COVERAGE.iter().any(|c| c.path == path && present.iter().all(|a| c.args.contains(a)))
}

/// Stable identity of the table, recorded in docs/evidence at each sprint end
/// (and printed by `lope status` once that is native in S1).
pub fn fingerprint() -> String {
    let mut canonical = String::new();
    for c in COVERAGE {
        canonical.push_str(&c.path.join(" "));
        canonical.push('|');
        let mut args: Vec<&str> = c.args.to_vec();
        args.sort_unstable();
        canonical.push_str(&args.join(","));
        canonical.push('\n');
    }
    let h = canonical.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("fnv64:{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar;

    #[test]
    fn every_entry_exists_in_the_python_grammar() {
        for c in COVERAGE {
            let mut spec = &grammar::ROOT;
            for name in c.path {
                spec = spec.subcommand(name).unwrap_or_else(|| panic!("{:?} not in grammar", c.path));
                assert_eq!(spec.name, *name, "coverage must use canonical names, not aliases");
            }
            for a in c.args {
                let known = spec.options.iter().any(|o| o.id() == *a) || spec.positionals.iter().any(|p| p.dest == *a);
                assert!(known, "{:?}: unknown arg {a}", c.path);
            }
        }
    }

    #[test]
    fn version_is_native_only_without_arguments() {
        assert!(is_covered(&["version"], &[]));
        assert!(!is_covered(&["version"], &["--help"]));
        assert!(!is_covered(&["review"], &[]));
        assert!(is_covered(&["jobs", "kill"], &["run_id", "--json"]));
    }
}
