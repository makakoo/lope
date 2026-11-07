//! Filesystem locations shared with Python Lope.

use std::ffi::OsString;
use std::path::PathBuf;

/// `$LOPE_HOME`, else `~/.lope`, with `~` expanded (`jobs.py:55-56`).
/// An empty `LOPE_HOME` counts as unset, as in Python's `or`.
pub fn lope_home() -> PathBuf {
    lope_home_from(std::env::var_os("LOPE_HOME"), std::env::var_os("HOME"))
}

pub fn lope_home_from(lope_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    let home = home
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    match lope_home.filter(|v| !v.is_empty()) {
        Some(v) => expand_user(PathBuf::from(v), &home),
        None => home.join(".lope"),
    }
}

/// Python's `Path.expanduser()` for the current user (`~` and `~/...` only).
fn expand_user(path: PathBuf, home: &std::path::Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lope_home_rules() {
        let h = Some(OsString::from("/Users/u"));
        assert_eq!(
            lope_home_from(None, h.clone()),
            PathBuf::from("/Users/u/.lope")
        );
        assert_eq!(
            lope_home_from(Some("".into()), h.clone()),
            PathBuf::from("/Users/u/.lope")
        );
        assert_eq!(
            lope_home_from(Some("/opt/l".into()), h.clone()),
            PathBuf::from("/opt/l")
        );
        assert_eq!(
            lope_home_from(Some("~/x".into()), h.clone()),
            PathBuf::from("/Users/u/x")
        );
        assert_eq!(
            lope_home_from(Some("~".into()), h),
            PathBuf::from("/Users/u")
        );
    }
}
