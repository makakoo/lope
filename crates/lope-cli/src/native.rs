//! Native command implementations, dispatched by canonical command path. Only
//! paths listed in lope_compat::coverage reach this module.

use lope_compat::parser::Invocation;
use std::io::Write;

pub fn run(inv: &Invocation) -> i32 {
    match inv.path.as_slice() {
        ["version"] => version(),
        other => {
            unreachable!("coverage table lists {other:?} but lope-cli has no native implementation")
        }
    }
}

/// FR2: exit 0; the first `N.N[.N]` on stdout is the product version.
fn version() -> i32 {
    let mut out = std::io::stdout().lock();
    match writeln!(out, "{}", lope_core::version_line()).and_then(|_| out.flush()) {
        Ok(()) => 0,
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_covered_path_has_an_implementation() {
        for c in lope_compat::coverage::COVERAGE {
            assert!(
                matches!(c.path, ["version"]),
                "add a native implementation for {:?}",
                c.path
            );
        }
    }
}
