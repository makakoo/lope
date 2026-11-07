//! `lope`: Super Lope entry point. Every invocation goes through the strangler
//! router (DEC-1); what is not native yet is exec'd into Python Lope.

mod native;

use lope_compat::route::{self, Route, RouterEnv};
use lope_compat::{delegate, normalize::Rewrites};
use std::ffi::OsString;
use std::process::exit;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (route, rewrites) = route::route(args, &RouterEnv::from_process());
    if lope_core::env::flag("LOPE_ROUTER_TRACE") {
        trace(&route, &rewrites);
    }
    let code = match route {
        Route::Native { invocation, .. } => native::run(&invocation),
        Route::Delegate { args, .. } => delegate::exec(&args),
        Route::Exit { code, stderr } => {
            eprintln!("{stderr}");
            code
        }
    };
    exit(code);
}

/// `LOPE_ROUTER_TRACE=1`: one line on stderr describing the routing decision.
fn trace(route: &Route, rw: &Rewrites) {
    let mut notes = Vec::new();
    if rw.version_flag {
        notes.push("version-flag".to_string());
    }
    if rw.consensus_quorum > 0 {
        notes.push(format!("consensus-quorum×{}", rw.consensus_quorum));
    }
    if rw.negotiate_rounds > 0 {
        notes.push(format!("negotiate-rounds×{}", rw.negotiate_rounds));
    }
    let notes = if notes.is_empty() {
        String::new()
    } else {
        format!(" [normalised: {}]", notes.join(", "))
    };
    match route {
        Route::Native { invocation, .. } => {
            eprintln!("lope-router: native {}{notes}", invocation.path.join(" "))
        }
        Route::Delegate { args, reason } => {
            let argv: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            eprintln!(
                "lope-router: delegate ({reason:?}) {} -m lope {}{notes}",
                delegate::interpreter().to_string_lossy(),
                argv.join(" ")
            );
        }
        Route::Exit { code, .. } => eprintln!("lope-router: exit {code}{notes}"),
    }
}
