//! JSON text that is byte-identical to CPython's `json.dumps`.
//!
//! Python Lope writes state with several `json.dumps` flavours (compact
//! default separators in `config.json`, `sort_keys` + `(",", ":")` in run
//! manifests, `indent=2, sort_keys=True` for `--json` output). Rust must
//! produce the same bytes so that files stay interchangeable and outputs stay
//! diff-identical. Values come from `serde_json` with `preserve_order`, so key
//! order is the insertion order, as with Python dicts.

use serde_json::{Map, Number, Value};
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct Opts {
    pub indent: Option<usize>,
    pub sort_keys: bool,
    pub ensure_ascii: bool,
    /// `(item_separator, key_separator)`. Python's default is `(", ", ": ")`,
    /// or `(",", ": ")` when `indent` is set.
    pub separators: Option<(&'static str, &'static str)>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts { indent: None, sort_keys: false, ensure_ascii: true, separators: None }
    }
}

impl Opts {
    /// `json.dumps(x)`
    pub fn default_dumps() -> Self {
        Self::default()
    }
    /// `json.dumps(x, sort_keys=True, separators=(",", ":"))` (run manifests)
    pub fn compact_sorted() -> Self {
        Opts { sort_keys: true, separators: Some((",", ":")), ..Self::default() }
    }
    /// `json.dumps(x, indent=2, sort_keys=True)` (`print_json`)
    pub fn pretty_sorted() -> Self {
        Opts { indent: Some(2), sort_keys: true, ..Self::default() }
    }
    /// `json.dumps(x, indent=2)`
    pub fn pretty() -> Self {
        Opts { indent: Some(2), ..Self::default() }
    }
}

pub fn dumps(v: &Value, opts: &Opts) -> String {
    let (item_sep, key_sep) = opts.separators.unwrap_or(if opts.indent.is_some() { (",", ": ") } else { (", ", ": ") });
    let mut out = String::new();
    write_value(&mut out, v, opts, item_sep, key_sep, 0);
    out
}

fn newline_indent(out: &mut String, opts: &Opts, level: usize) {
    if let Some(n) = opts.indent {
        out.push('\n');
        for _ in 0..n * level {
            out.push(' ');
        }
    }
}

fn write_value(out: &mut String, v: &Value, opts: &Opts, item_sep: &str, key_sep: &str, level: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => write_str(out, s, opts.ensure_ascii),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(item_sep);
                }
                newline_indent(out, opts, level + 1);
                write_value(out, item, opts, item_sep, key_sep, level + 1);
            }
            newline_indent(out, opts, level);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            if opts.sort_keys {
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            for (i, (k, val)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(item_sep);
                }
                newline_indent(out, opts, level + 1);
                write_str(out, k, opts.ensure_ascii);
                out.push_str(key_sep);
                write_value(out, val, opts, item_sep, key_sep, level + 1);
            }
            newline_indent(out, opts, level);
            out.push('}');
        }
    }
}

fn number(n: &Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    float_json(n.as_f64().unwrap_or(f64::NAN))
}

/// `json` encodes floats with `float.__repr__`, except NaN/±inf.
pub fn float_json(f: f64) -> String {
    if f.is_nan() {
        "NaN".into()
    } else if f.is_infinite() {
        if f > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        }
    } else {
        float_repr(f)
    }
}

/// CPython `repr(float)`: the shortest round-tripping digits, in fixed
/// notation when the decimal exponent is in [-4, 16), else scientific with a
/// signed, at-least-two-digit exponent. Fixed notation always has a `.`.
pub fn float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // Rust's `{:e}` gives a shortest round-trip digit string, but on exact
    // ties between two shortest candidates it does not pick the even one as
    // CPython (David Gay, mode 0) does. Re-format at that length with exact,
    // half-even rounding and keep it if it still round-trips.
    let shortest = format!("{:e}", f.abs());
    let ndigits = shortest.split_once('e').expect("exponent").0.chars().filter(|c| c.is_ascii_digit()).count();
    let rounded = format!("{:.*e}", ndigits - 1, f.abs());
    let sci = if rounded.parse::<f64>().ok() == Some(f.abs()) { rounded } else { shortest };
    let (mant, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent int");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let decpt = exp + 1; // value = 0.DIGITS × 10^decpt
    let sign = if f < 0.0 { "-" } else { "" };
    let mut s = String::new();
    s.push_str(sign);
    if -4 < decpt && decpt <= 16 {
        if decpt <= 0 {
            s.push_str("0.");
            for _ in 0..(-decpt) {
                s.push('0');
            }
            s.push_str(&digits);
        } else {
            let d = decpt as usize;
            if digits.len() <= d {
                s.push_str(&digits);
                for _ in 0..(d - digits.len()) {
                    s.push('0');
                }
                s.push_str(".0");
            } else {
                s.push_str(&digits[..d]);
                s.push('.');
                s.push_str(&digits[d..]);
            }
        }
    } else {
        s.push_str(&digits[..1]);
        if digits.len() > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        let e = decpt - 1;
        let _ = write!(s, "e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    s
}

fn write_str(out: &mut String, s: &str, ensure_ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c if ensure_ascii && (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{:04x}", unit);
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `json.loads(text)`: see [`loads_py`].
pub fn loads(text: &str) -> Result<Value, PyJsonError> {
    loads_py(text)
}

/// Convenience: an empty JSON object.
pub fn object() -> Map<String, Value> {
    Map::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn float_repr_matches_cpython() {
        let cases = [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1.5, "1.5"),
            (100.0, "100.0"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.234e16, "1.234e+16"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (123456.789, "123456.789"),
            (1794057125.7847402, "1794057125.7847402"),
            (2.5e-300, "2.5e-300"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (0.85, "0.85"),
            (-3.25, "-3.25"),
            (5.709, "5.709"),
            #[allow(clippy::excessive_precision)]
            (9725986424039.0625, "9725986424039.062"),
            (0.125, "0.125"),
        ];
        for (f, want) in cases {
            assert_eq!(float_repr(f), want, "{f:?}");
        }
    }

    #[test]
    fn dumps_flavours() {
        let v: Value = loads(r#"{"b": [1, 2.0, "é\n"], "a": {"x": null, "y": true}, "e": {}, "l": []}"#).unwrap();
        assert_eq!(
            dumps(&v, &Opts::default()),
            r#"{"b": [1, 2.0, "\u00e9\n"], "a": {"x": null, "y": true}, "e": {}, "l": []}"#
        );
        assert_eq!(
            dumps(&v, &Opts::compact_sorted()),
            r#"{"a":{"x":null,"y":true},"b":[1,2.0,"\u00e9\n"],"e":{},"l":[]}"#
        );
        assert_eq!(
            dumps(&v, &Opts::pretty_sorted()),
            "{\n  \"a\": {\n    \"x\": null,\n    \"y\": true\n  },\n  \"b\": [\n    1,\n    2.0,\n    \"\\u00e9\\n\"\n  ],\n  \"e\": {},\n  \"l\": []\n}"
        );
        let astral = json!("😀\u{7f}\u{1}");
        assert_eq!(dumps(&astral, &Opts::default()), "\"\\ud83d\\ude00\\u007f\\u0001\"");
        let raw = Opts { ensure_ascii: false, ..Opts::default() };
        assert_eq!(dumps(&json!("é😀\u{7f}"), &raw), "\"é😀\u{7f}\"");
    }

    #[test]
    fn duplicate_keys_keep_first_position_last_value() {
        let v = loads(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap();
        assert_eq!(dumps(&v, &Opts::default()), r#"{"a": 3, "b": 2}"#);
    }
}

/// A `json.JSONDecodeError`, formatted as Python formats it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyJsonError {
    pub msg: &'static str,
    /// Character (not byte) offset, as in Python.
    pub pos: usize,
    pub lineno: usize,
    pub colno: usize,
}

impl std::fmt::Display for PyJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: line {} column {} (char {})", self.msg, self.lineno, self.colno, self.pos)
    }
}
impl std::error::Error for PyJsonError {}

/// `json.loads(text)` with CPython's C-scanner semantics and error messages
/// (`Modules/_json.c`, CPython 3.9). NaN/±Infinity literals and floats that
/// overflow decode to `null` (serde_json cannot hold them).
pub fn loads_py(text: &str) -> Result<Value, PyJsonError> {
    let chars: Vec<char> = text.chars().collect();
    let mut p = PyParser { s: &chars };
    let start = p.ws(0);
    let (v, end) = match p.scan_once(start) {
        Ok(x) => x,
        Err(Fail::Stop(i)) => return Err(p.err("Expecting value", i)),
        Err(Fail::Err(e)) => return Err(e),
    };
    let end = p.ws(end);
    if end != chars.len() {
        return Err(p.err("Extra data", end));
    }
    Ok(v)
}

enum Fail {
    Stop(usize),
    Err(PyJsonError),
}

struct PyParser<'a> {
    s: &'a [char],
}

impl PyParser<'_> {
    fn err(&self, msg: &'static str, pos: usize) -> PyJsonError {
        let before = &self.s[..pos.min(self.s.len())];
        let lineno = before.iter().filter(|c| **c == '\n').count() + 1;
        let colno = match before.iter().rposition(|c| *c == '\n') {
            Some(nl) => pos - nl,
            None => pos + 1,
        };
        PyJsonError { msg, pos, lineno, colno }
    }

    fn ws(&self, mut i: usize) -> usize {
        while i < self.s.len() && matches!(self.s[i], ' ' | '\t' | '\n' | '\r') {
            i += 1;
        }
        i
    }

    fn starts(&self, i: usize, lit: &str) -> bool {
        let l: Vec<char> = lit.chars().collect();
        self.s.len() >= i + l.len() && self.s[i..i + l.len()] == l[..]
    }

    fn scan_once(&mut self, i: usize) -> Result<(Value, usize), Fail> {
        if i >= self.s.len() {
            return Err(Fail::Stop(i));
        }
        match self.s[i] {
            '"' => self.string(i + 1).map(|(s, e)| (Value::String(s), e)).map_err(Fail::Err),
            '{' => self.object(i + 1),
            '[' => self.array(i + 1),
            'n' if self.starts(i, "null") => Ok((Value::Null, i + 4)),
            't' if self.starts(i, "true") => Ok((Value::Bool(true), i + 4)),
            'f' if self.starts(i, "false") => Ok((Value::Bool(false), i + 5)),
            'N' if self.starts(i, "NaN") => Ok((Value::Null, i + 3)),
            'I' if self.starts(i, "Infinity") => Ok((Value::Null, i + 8)),
            '-' if self.starts(i, "-Infinity") => Ok((Value::Null, i + 9)),
            _ => self.number(i),
        }
    }

    fn number(&self, start: usize) -> Result<(Value, usize), Fail> {
        let s = self.s;
        let n = s.len();
        let mut i = start;
        if i < n && s[i] == '-' {
            i += 1;
            if i >= n {
                return Err(Fail::Stop(start));
            }
        }
        if i < n && ('1'..='9').contains(&s[i]) {
            i += 1;
            while i < n && s[i].is_ascii_digit() {
                i += 1;
            }
        } else if i < n && s[i] == '0' {
            i += 1;
        } else {
            return Err(Fail::Stop(start));
        }
        let mut is_float = false;
        if i < n - 1 && s[i] == '.' && s[i + 1].is_ascii_digit() {
            is_float = true;
            i += 2;
            while i < n && s[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i < n - 1 && (s[i] == 'e' || s[i] == 'E') {
            let e_start = i;
            i += 1;
            if i < n - 1 && (s[i] == '-' || s[i] == '+') {
                i += 1;
            }
            while i < n && s[i].is_ascii_digit() {
                i += 1;
            }
            if s[i - 1].is_ascii_digit() {
                is_float = true;
            } else {
                i = e_start;
            }
        }
        let text: String = s[start..i].iter().collect();
        let v = if is_float {
            text.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null)
        } else if let Ok(x) = text.parse::<i64>() {
            Value::from(x)
        } else if let Ok(x) = text.parse::<u64>() {
            Value::from(x)
        } else {
            text.parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null)
        };
        Ok((v, i))
    }

    /// `scanstring_unicode(pystr, end, strict=1)`; `end` is just past the quote.
    fn string(&self, end: usize) -> Result<(String, usize), PyJsonError> {
        let s = self.s;
        let begin = end - 1;
        let mut out = String::new();
        let mut i = end;
        loop {
            // find the next quote, backslash or control char
            let mut next = i;
            while next < s.len() {
                let c = s[next];
                if c == '"' || c == '\\' {
                    break;
                }
                if (c as u32) <= 0x1f {
                    return Err(self.err("Invalid control character at", next));
                }
                next += 1;
            }
            if next >= s.len() {
                return Err(self.err("Unterminated string starting at", begin));
            }
            out.extend(&s[i..next]);
            let c = s[next];
            next += 1;
            if c == '"' {
                return Ok((out, next));
            }
            if next >= s.len() {
                return Err(self.err("Unterminated string starting at", begin));
            }
            let esc = s[next];
            if esc != 'u' {
                let decoded = match esc {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\u{08}',
                    'f' => '\u{0c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return Err(self.err("Invalid \\escape", next - 1)),
                };
                out.push(decoded);
                i = next + 1;
                continue;
            }
            // \uXXXX
            next += 1;
            let hex = |at: usize| -> Option<u32> {
                if at + 4 > s.len() {
                    return None;
                }
                let t: String = s[at..at + 4].iter().collect();
                if t.chars().all(|c| c.is_ascii_hexdigit()) {
                    u32::from_str_radix(&t, 16).ok()
                } else {
                    None
                }
            };
            // CPython: `end >= len` (the escape may not end the document)
            if next + 4 >= s.len() {
                return Err(self.err("Invalid \\uXXXX escape", next - 1));
            }
            let Some(mut cp) = hex(next) else {
                return Err(self.err("Invalid \\uXXXX escape", next - 1));
            };
            next += 4;
            if (0xd800..=0xdbff).contains(&cp) && next + 6 < s.len() && s[next] == '\\' && s[next + 1] == 'u' {
                let Some(lo) = hex(next + 2) else {
                    return Err(self.err("Invalid \\uXXXX escape", next + 1));
                };
                if (0xdc00..=0xdfff).contains(&lo) {
                    cp = 0x10000 + (((cp - 0xd800) << 10) | (lo - 0xdc00));
                    next += 6;
                }
            }
            // Lone surrogates cannot live in a Rust String: substitute U+FFFD.
            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
            i = next;
        }
    }

    fn object(&mut self, start: usize) -> Result<(Value, usize), Fail> {
        let mut m = Map::new();
        let mut i = self.ws(start);
        if i < self.s.len() && self.s[i] == '}' {
            return Ok((Value::Object(m), i + 1));
        }
        loop {
            if i >= self.s.len() || self.s[i] != '"' {
                return Err(Fail::Err(self.err("Expecting property name enclosed in double quotes", i)));
            }
            let (key, after) = self.string(i + 1).map_err(Fail::Err)?;
            i = self.ws(after);
            if i >= self.s.len() || self.s[i] != ':' {
                return Err(Fail::Err(self.err("Expecting ':' delimiter", i)));
            }
            i = self.ws(i + 1);
            let (v, after) = self.scan_once(i)?;
            m.insert(key, v);
            i = self.ws(after);
            if i < self.s.len() && self.s[i] == '}' {
                return Ok((Value::Object(m), i + 1));
            }
            if i >= self.s.len() || self.s[i] != ',' {
                return Err(Fail::Err(self.err("Expecting ',' delimiter", i)));
            }
            i = self.ws(i + 1);
        }
    }

    fn array(&mut self, start: usize) -> Result<(Value, usize), Fail> {
        let mut a = Vec::new();
        let mut i = self.ws(start);
        if i < self.s.len() && self.s[i] == ']' {
            return Ok((Value::Array(a), i + 1));
        }
        loop {
            let (v, after) = self.scan_once(i)?;
            a.push(v);
            i = self.ws(after);
            if i < self.s.len() && self.s[i] == ']' {
                return Ok((Value::Array(a), i + 1));
            }
            if i >= self.s.len() || self.s[i] != ',' {
                return Err(Fail::Err(self.err("Expecting ',' delimiter", i)));
            }
            i = self.ws(i + 1);
        }
    }
}
