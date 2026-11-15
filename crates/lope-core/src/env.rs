//! Environment conventions shared with Python Lope.

/// Python Lope's boolean env rule: `value.lower() in ("1", "true", "yes", "on")`
/// (`config.py:210`). Surrounding whitespace is ignored here; Python's
/// call sites differ on that, and these flags are Super Lope's own.
pub fn truthy(value: Option<&str>) -> bool {
    match value {
        Some(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        None => false,
    }
}

pub fn flag(name: &str) -> bool {
    truthy(std::env::var(name).ok().as_deref())
}

/// Message printed by the nesting guard (FR4, `cli.py:189`), byte for byte.
pub const NESTED_MESSAGE: &str = "error: nested Lope orchestration is disabled inside an implementation writer";

/// FR4: `LOPE_IMPLEMENTATION_DEPTH` (default `"0"`) means "nested" when Python's
/// `int(value) > 0`, or when `int()` raises.
///
/// Mirrors CPython's `int(str)` for ASCII input: surrounding whitespace, an
/// optional sign, and single underscores between digits. Non-ASCII digits,
/// which CPython also accepts, are treated as invalid, so they count as nested.
pub fn implementation_depth_is_nested(value: Option<&str>) -> bool {
    let Some(raw) = value else { return false };
    match python_int_sign(raw) {
        Some(sign) => sign > 0,
        None => true,
    }
}

/// Returns the sign (-1, 0, 1) of `int(raw)`, or `None` if `int()` would raise.
fn python_int_sign(raw: &str) -> Option<i8> {
    let s = raw.trim_matches(|c: char| c.is_whitespace());
    let (negative, digits) = match s.as_bytes().first()? {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    let bytes = digits.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_digit() || !bytes[bytes.len() - 1].is_ascii_digit() {
        return None;
    }
    let mut nonzero = false;
    let mut prev_underscore = false;
    for &b in bytes {
        match b {
            b'0'..=b'9' => {
                nonzero |= b != b'0';
                prev_underscore = false;
            }
            b'_' if !prev_underscore => prev_underscore = true,
            _ => return None,
        }
    }
    Some(match (nonzero, negative) {
        (false, _) => 0,
        (true, false) => 1,
        (true, true) => -1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_matches_python_int_semantics() {
        let nested = |v: &str| implementation_depth_is_nested(Some(v));
        assert!(!implementation_depth_is_nested(None));
        for v in ["0", "00", " 0 ", "-1", "-0", "+0", "0_0", "\t0\n"] {
            assert!(!nested(v), "{v:?} should not be nested");
        }
        for v in ["1", " 2 ", "+3", "1_0", "", " ", "x", "1.0", "1__0", "_1", "1_", "0x1", "--1", "١"] {
            assert!(nested(v), "{v:?} should be nested");
        }
    }

    #[test]
    fn truthy_follows_python_set() {
        for v in ["1", "true", "TRUE", "yes", "On", " on "] {
            assert!(truthy(Some(v)), "{v:?}");
        }
        for v in ["0", "false", "", "no", "2"] {
            assert!(!truthy(Some(v)), "{v:?}");
        }
        assert!(!truthy(None));
    }
}
