//! The strangler router (FR5, DEC-1, Spec 01 §5).
//!
//! Order: nesting guard → normalisation → forced delegation → coverage
//! lookup → native or delegated. The guard runs first, exactly as in
//! `cli.py:182-192`, so a nested call never reaches argument handling.

use crate::{coverage, normalize, parser};
use lope_core::{env, exit};
use std::ffi::OsString;

/// Router inputs taken from the environment (injected for tests).
#[derive(Debug, Clone, Default)]
pub struct RouterEnv {
    pub implementation_depth: Option<String>,
    pub force_python: bool,
    pub force_native: bool,
}

impl RouterEnv {
    pub fn from_process() -> Self {
        RouterEnv {
            implementation_depth: std::env::var_os("LOPE_IMPLEMENTATION_DEPTH")
                .map(|v| v.to_string_lossy().into_owned()),
            force_python: env::flag("LOPE_FORCE_PYTHON"),
            force_native: env::flag("LOPE_FORCE_NATIVE"),
        }
    }
}

#[derive(Debug)]
pub enum Route {
    /// Run natively. `args` is the normalised argv (without program name).
    Native { invocation: parser::Invocation, args: Vec<OsString> },
    /// Exec Python Lope with the normalised argv.
    Delegate { args: Vec<OsString>, reason: DelegateReason },
    /// Print `stderr` and exit with `code`, without running anything.
    Exit { code: i32, stderr: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegateReason {
    ForcedPython,
    NoCoverage,
    /// clap rejected the argv (usage error, `--help`, …): Python owns the message.
    ParseError,
}

pub fn route(args: Vec<OsString>, renv: &RouterEnv) -> (Route, normalize::Rewrites) {
    if env::implementation_depth_is_nested(renv.implementation_depth.as_deref()) {
        let r = Route::Exit { code: exit::USAGE, stderr: env::NESTED_MESSAGE.to_string() };
        return (r, Default::default());
    }
    let (args, rewrites) = match normalize::normalize(args) {
        Ok(x) => x,
        Err(e) => return (Route::Exit { code: exit::USAGE, stderr: e.to_string() }, Default::default()),
    };
    if renv.force_python {
        return (Route::Delegate { args, reason: DelegateReason::ForcedPython }, rewrites);
    }
    let verb = args.first().and_then(|a| a.to_str()).unwrap_or("");
    let reason = if !coverage::verb_has_coverage(verb) {
        DelegateReason::NoCoverage
    } else {
        match parser::parse(&args) {
            Ok(inv) => {
                let present: Vec<&str> = inv.present.iter().map(String::as_str).collect();
                if coverage::is_covered(&inv.path, &present) {
                    return (Route::Native { invocation: inv, args }, rewrites);
                }
                DelegateReason::NoCoverage
            }
            Err(_) => DelegateReason::ParseError,
        }
    };
    if renv.force_native {
        let what =
            if verb.is_empty() { "this invocation".to_string() } else { format!("`lope {verb}` with these arguments") };
        let r = Route::Exit {
            code: exit::USAGE,
            stderr: format!("lope: error: {what} is not implemented natively yet (LOPE_FORCE_NATIVE=1)"),
        };
        return (r, rewrites);
    }
    (Route::Delegate { args, reason }, rewrites)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(args: &[&str], renv: &RouterEnv) -> Route {
        route(args.iter().map(OsString::from).collect(), renv).0
    }

    #[test]
    fn version_is_native() {
        let e = RouterEnv::default();
        assert!(matches!(r(&["version"], &e), Route::Native { .. }));
        assert!(matches!(r(&["--version"], &e), Route::Native { .. }));
        assert!(matches!(r(&["-V"], &e), Route::Native { .. }));
        assert!(matches!(r(&["version", "--help"], &e), Route::Delegate { reason: DelegateReason::ParseError, .. }));
        assert!(matches!(r(&["version", "extra"], &e), Route::Delegate { reason: DelegateReason::ParseError, .. }));
    }

    #[test]
    fn everything_else_delegates_with_normalised_argv() {
        let e = RouterEnv::default();
        match r(&["review", "n.md", "--consensus", "quorum"], &e) {
            Route::Delegate { args, reason } => {
                assert_eq!(reason, DelegateReason::NoCoverage);
                assert_eq!(args, ["review", "n.md", "--consensus"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(r(&[], &e), Route::Delegate { .. }));
        assert!(matches!(r(&["-h"], &e), Route::Delegate { .. }));
    }

    #[test]
    fn nesting_guard_comes_first() {
        let e = RouterEnv { implementation_depth: Some("1".into()), ..Default::default() };
        for args in [&["version"][..], &["review", "f", "--consensus", "any"], &[]] {
            match r(args, &e) {
                Route::Exit { code, stderr } => {
                    assert_eq!(code, 2);
                    assert_eq!(stderr, env::NESTED_MESSAGE);
                }
                other => panic!("{other:?}"),
            }
        }
        let e = RouterEnv { implementation_depth: Some("0".into()), ..Default::default() };
        assert!(matches!(r(&["version"], &e), Route::Native { .. }));
    }

    #[test]
    fn force_flags() {
        let py = RouterEnv { force_python: true, ..Default::default() };
        assert!(matches!(r(&["version"], &py), Route::Delegate { reason: DelegateReason::ForcedPython, .. }));
        let native = RouterEnv { force_native: true, ..Default::default() };
        assert!(matches!(r(&["version"], &native), Route::Native { .. }));
        assert!(matches!(r(&["status"], &native), Route::Exit { code: 2, .. }));
        // Python wins when both are set (rollback safety).
        let both = RouterEnv { force_python: true, force_native: true, ..Default::default() };
        assert!(matches!(r(&["status"], &both), Route::Delegate { .. }));
    }

    #[test]
    fn oq1_modes_exit_2() {
        match r(&["review", "f.md", "--consensus", "unanimous"], &RouterEnv::default()) {
            Route::Exit { code, stderr } => {
                assert_eq!(code, 2);
                assert!(stderr.starts_with("lope review: error: --consensus unanimous"), "{stderr}");
            }
            other => panic!("{other:?}"),
        }
    }
}
