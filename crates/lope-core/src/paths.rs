//! Filesystem locations shared with Python Lope.

use std::ffi::OsString;
use std::path::PathBuf;

/// `$LOPE_HOME`, else `~/.lope`, with `~` expanded (`jobs.py:55-56`).
/// An empty `LOPE_HOME` counts as unset, as in Python's `or`.
pub fn lope_home() -> PathBuf {
    lope_home_from(std::env::var_os("LOPE_HOME"), std::env::var_os("HOME"))
}

pub fn lope_home_from(lope_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    let home = home.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    match lope_home.filter(|v| !v.is_empty()) {
        Some(v) => expand_user(PathBuf::from(v), &home),
        None => home.join(".lope"),
    }
}

/// Python's `Path.expanduser()` for the current user (`~` and `~/...` only).
pub fn expand_user(path: PathBuf, home: &std::path::Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path,
    }
}

/// Expand a leading `~` or `~/...` path using `$HOME` / passwd fallback.
pub fn expanduser(raw: &str) -> PathBuf {
    expand_user(PathBuf::from(raw), &home_dir())
}

/// `os.environ.get("LOPE_HOME", os.path.expanduser("~/.lope"))`: the form
/// used by `config.py`, `journal.py`, `latency.py`, `trust.py`, `auditor.py`.
/// Unlike [`lope_home`], a set-but-empty LOPE_HOME stays empty (paths become
/// cwd-relative) and no `~` expansion is applied (Audit 01 §4, finding F3).
pub fn lope_home_get_default() -> PathBuf {
    match std::env::var_os("LOPE_HOME") {
        Some(v) => PathBuf::from(v),
        None => home_dir().join(".lope"),
    }
}

/// `os.path.expanduser("~")`: $HOME, else the passwd entry.
pub fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h);
    }
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_dir.is_null() {
            let s = std::ffi::CStr::from_ptr((*pw).pw_dir);
            return PathBuf::from(std::ffi::OsStr::from_encoded_bytes_unchecked(s.to_bytes()));
        }
    }
    PathBuf::from("/")
}

/// `shutil.which(name)`: first executable regular file on $PATH (or the
/// name itself if it contains a `/`).
pub fn which(name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let is_exec = |p: &std::path::Path| {
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
            && unsafe {
                let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap_or_default();
                libc::access(c.as_ptr(), libc::X_OK) == 0
            }
    };
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_exec(&p).then_some(p);
    }
    let path = std::env::var_os("PATH").unwrap_or_else(|| std::ffi::OsString::from("/bin:/usr/bin"));
    for dir in std::env::split_paths(&path) {
        let dir = if dir.as_os_str().is_empty() { PathBuf::from(".") } else { dir };
        let p = dir.join(name);
        if is_exec(&p) {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lope_home_rules() {
        let h = Some(OsString::from("/Users/u"));
        assert_eq!(lope_home_from(None, h.clone()), PathBuf::from("/Users/u/.lope"));
        assert_eq!(lope_home_from(Some("".into()), h.clone()), PathBuf::from("/Users/u/.lope"));
        assert_eq!(lope_home_from(Some("/opt/l".into()), h.clone()), PathBuf::from("/opt/l"));
        assert_eq!(lope_home_from(Some("~/x".into()), h.clone()), PathBuf::from("/Users/u/x"));
        assert_eq!(lope_home_from(Some("~".into()), h), PathBuf::from("/Users/u"));
    }
}
