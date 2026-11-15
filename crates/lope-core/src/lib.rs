//! Shared foundations: paths, version constants, exit codes and environment
//! conventions that every other crate follows (Spec 01 §3).
//!
//! S0 provides only what the router needs. Config, models, redaction and the
//! verdict parser arrive in S1.

pub mod config;
pub mod env;
pub mod exit;
pub mod journal;
pub mod logo;
pub mod output;
pub mod paths;
pub mod pyjson;
pub mod redaction;
pub mod time;

/// Product version (OQ-3 default: `2.0.0`).
pub const PRODUCT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Python Lope release this build is parity-tested against (the PL-1 pin).
pub const COMPAT_VERSION: &str = "0.17.3";

/// The line `lope version` prints (FR2). The first `N.N[.N]` on stdout must be
/// the product version, so nothing version-like may precede it.
pub fn version_line() -> String {
    format!("lope {PRODUCT_VERSION} (super-lope; compat {COMPAT_VERSION})")
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Glimmer's doctor regex `/v?([0-9]+\.[0-9]+(?:\.[0-9]+)?)/` must pick the
    /// product version, not the compat version.
    #[test]
    fn first_version_match_is_product_version() {
        let line = version_line();
        let first = line
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .find(|t| t.contains('.') && t.split('.').filter(|p| !p.is_empty()).count() >= 2)
            .unwrap();
        assert_eq!(first, PRODUCT_VERSION);
    }
}
