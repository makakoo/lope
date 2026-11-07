//! Native command implementations, dispatched by canonical command path. Only
//! paths listed in lope_compat::coverage reach this module.

mod discover;
mod jobs;
mod status;

use lope_compat::parser::Invocation;
use std::io::Write;

/// Buffered stdout that behaves like Python's `print` at exit: write errors
/// (including EPIPE) are ignored.
pub struct Out {
    buf: Vec<u8>,
}

impl Out {
    pub fn new() -> Self {
        Out { buf: Vec::new() }
    }
    pub fn line(&mut self, s: impl AsRef<str>) {
        self.buf.extend_from_slice(s.as_ref().as_bytes());
        self.buf.push(b'\n');
    }
    pub fn blank(&mut self) {
        self.buf.push(b'\n');
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        let mut so = std::io::stdout().lock();
        let _ = so.write_all(&self.buf);
        let _ = so.flush();
    }
}

pub fn run(inv: &Invocation) -> i32 {
    match inv.path.as_slice() {
        ["version"] => version(),
        ["status"] => status::run(),
        ["jobs"] => jobs::run(None, &inv.matches),
        ["jobs", sub] => jobs::run(Some(sub), &inv.matches),
        other => {
            unreachable!("coverage table lists {other:?} but lope-cli has no native implementation")
        }
    }
}

/// FR2: exit 0; the first `N.N[.N]` on stdout is the product version.
fn version() -> i32 {
    let mut out = Out::new();
    out.line(lope_core::version_line());
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_covered_path_has_an_implementation() {
        for c in lope_compat::coverage::COVERAGE {
            assert!(
                matches!(c.path, ["version"] | ["status"] | ["jobs"] | ["jobs", "list" | "reap" | "prune" | "kill"]),
                "add a native implementation for {:?}",
                c.path
            );
        }
    }
}
