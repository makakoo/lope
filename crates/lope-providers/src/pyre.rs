//! Python `re` patterns compiled with fancy-regex (lookaround support).
//! Translation notes: Python `\Z` is `\z` here; Python's non-MULTILINE `$`
//! also matches before a final newline, which these patterns never rely on.

use fancy_regex::Regex;

pub fn re(p: &str) -> Regex {
    Regex::new(p).unwrap_or_else(|e| panic!("bad pattern {p:?}: {e}"))
}

pub fn search<'t>(r: &Regex, t: &'t str) -> Option<fancy_regex::Captures<'t>> {
    r.captures(t).ok().flatten()
}

pub fn is_match(r: &Regex, t: &str) -> bool {
    r.is_match(t).unwrap_or(false)
}

/// Python `str.splitlines()` (no keepends): splits on \n, \r\n, \r, \v, \f,
/// \x1c-\x1e, \x85,  ,  .
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let b: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < b.len() {
        let (pos, c) = b[i];
        let is_break =
            matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}');
        if is_break {
            out.push(&s[start..pos]);
            let mut next = pos + c.len_utf8();
            if c == '\r' && i + 1 < b.len() && b[i + 1].1 == '\n' {
                next += 1;
                i += 1;
            }
            start = next;
        }
        i += 1;
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Python `str.strip()` (Unicode whitespace).
pub fn strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '\x1c'..='\x1f'))
}

/// First `n` characters (Python `s[:n]`).
pub fn head(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Last `n` characters (Python `s[-n:]`).
pub fn tail(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    let (i, _) = s.char_indices().nth(count - n).unwrap();
    &s[i..]
}

pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitlines_like_python() {
        assert_eq!(splitlines("a\nb\r\nc\rd\n"), ["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n\nb"), ["a", "", "b"]);
        assert!(splitlines("").is_empty());
        assert_eq!(head("héllo", 2), "hé");
        assert_eq!(tail("héllo", 3), "llo");
    }
}
