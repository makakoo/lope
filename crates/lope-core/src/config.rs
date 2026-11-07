//! `config.json` with Python Lope's exact semantics (`config.py`, FR6).
//!
//! Validation follows Python's `isinstance` rules (a JSON bool counts as an
//! int; a float never does), values keep their original JSON form so a
//! load + save round trip is byte-identical to Python's `save()`, and only
//! Python's recognised keys are ever written (DEC-3).

use crate::pyjson::{self, Opts};
use serde_json::{Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const VERSION: i64 = 1;
pub const DEFAULT_MODEL_CALL_TIMEOUT_SECONDS: i64 = 960;
pub const DEFAULT_MAX_EXTERNAL_CALLS: i64 = 96;
pub const DEFAULT_MAX_INPUT_BYTES: i64 = 16 * 1024 * 1024;
pub const DEFAULT_MAX_OUTPUT_BYTES: i64 = 32 * 1024 * 1024;
pub const DEFAULT_MAX_CHUNKS: i64 = 32;

/// Python `isinstance(v, int)` for a JSON value (bools are ints).
pub fn py_int(v: &Value) -> Option<i64> {
    match v {
        Value::Bool(b) => Some(*b as i64),
        Value::Number(n) if n.is_i64() => n.as_i64(),
        Value::Number(n) if n.is_u64() => n.as_u64().map(|u| u as i64),
        _ => None,
    }
}

/// Python `float(v)` for the JSON values Lope stores (numbers, bools,
/// numeric strings). `None` where Python would raise.
pub fn py_float(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(*b as i64 as f64),
        Value::Number(n) => n.as_f64(),
        Value::String(s) => {
            let t = s.trim();
            match t.to_ascii_lowercase().as_str() {
                "inf" | "+inf" | "infinity" | "+infinity" => Some(f64::INFINITY),
                "-inf" | "-infinity" => Some(f64::NEG_INFINITY),
                "nan" | "+nan" | "-nan" => Some(f64::NAN),
                _ => t
                    .replace('_', "")
                    .parse::<f64>()
                    .ok()
                    .filter(|_| !t.starts_with('_') && !t.ends_with('_') && !t.contains("__")),
            }
        }
        _ => None,
    }
}

fn int_value(i: i64) -> Value {
    Value::from(i)
}

/// A self-healed validator invocation (`LearnedAdapter`).
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedAdapter {
    pub argv_template: Vec<String>,
    /// Kept as raw JSON: Python does not validate these fields' types.
    pub stdin_mode: Value,
    pub stdout_parser: Value,
    pub timestamp: f64,
    pub source_cli: Value,
    pub confidence: f64,
}

impl LearnedAdapter {
    pub fn source_cli_str(&self) -> String {
        match &self.source_cli {
            Value::String(s) => s.clone(),
            other => py_str(other),
        }
    }

    fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("argv_template".into(), Value::from(self.argv_template.clone()));
        m.insert("stdin_mode".into(), self.stdin_mode.clone());
        m.insert("stdout_parser".into(), self.stdout_parser.clone());
        m.insert("timestamp".into(), float_value(self.timestamp));
        m.insert("source_cli".into(), self.source_cli.clone());
        m.insert("confidence".into(), float_value(self.confidence));
        Value::Object(m)
    }
}

fn float_value(f: f64) -> Value {
    serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
}

/// Python `str(v)` for simple JSON values (used where Python formats raw
/// config values).
pub fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => pyjson::float_repr(n.as_f64().unwrap_or(f64::NAN)),
        },
        other => pyjson::dumps(other, &Opts::default()),
    }
}

/// `LopeCfg`. Integer-ish fields keep their JSON value so that `true` stays
/// `true` on save, exactly as in Python.
#[derive(Debug, Clone, PartialEq)]
pub struct LopeCfg {
    pub validators: Vec<Value>,
    pub primary: String,
    pub timeout: Value,
    pub parallel: bool,
    pub providers: Vec<Value>,
    /// Insertion-ordered (Python dict).
    pub learned_adapters: Vec<(String, LearnedAdapter)>,
    pub run_timeout: Option<Value>,
    pub allow_unbounded_run: bool,
    pub max_calls: Value,
    pub max_input_bytes: Value,
    pub max_output_bytes: Value,
    pub request_policy: String,
    pub max_chunks: Value,
}

impl LopeCfg {
    pub fn defaults() -> Self {
        LopeCfg {
            validators: vec![],
            primary: String::new(),
            timeout: int_value(DEFAULT_MODEL_CALL_TIMEOUT_SECONDS),
            parallel: true,
            providers: vec![],
            learned_adapters: vec![],
            run_timeout: None,
            allow_unbounded_run: false,
            max_calls: int_value(DEFAULT_MAX_EXTERNAL_CALLS),
            max_input_bytes: int_value(DEFAULT_MAX_INPUT_BYTES),
            max_output_bytes: int_value(DEFAULT_MAX_OUTPUT_BYTES),
            request_policy: "auto".into(),
            max_chunks: int_value(DEFAULT_MAX_CHUNKS),
        }
    }

    /// Validator names as strings (Python keeps whatever the list holds).
    pub fn validator_names(&self) -> Vec<String> {
        self.validators.iter().map(py_str).collect()
    }

    pub fn timeout_secs(&self) -> i64 {
        py_int(&self.timeout).unwrap_or(DEFAULT_MODEL_CALL_TIMEOUT_SECONDS)
    }

    pub fn run_timeout_secs(&self) -> Option<i64> {
        self.run_timeout.as_ref().and_then(py_int)
    }

    pub fn int_field(v: &Value) -> i64 {
        py_int(v).unwrap_or(0)
    }

    pub fn learned(&self, name: &str) -> Option<&LearnedAdapter> {
        self.learned_adapters.iter().find(|(n, _)| n == name).map(|(_, a)| a)
    }
}

/// `os.environ.get("LOPE_HOME", os.path.expanduser("~/.lope"))` (no `or`:
/// an empty LOPE_HOME stays empty) joined with `config.json`.
pub fn default_path() -> PathBuf {
    crate::paths::lope_home_get_default().join("config.json")
}

pub fn project_path(cwd: &Path) -> PathBuf {
    cwd.join(".lope").join("config.json")
}

/// `_safe_read`: missing → None; unreadable/invalid JSON → one retry after
/// 50 ms, then None.
pub fn safe_read(path: &Path) -> Option<Value> {
    for attempt in 0..2 {
        if !path.exists() {
            return None;
        }
        match std::fs::read(path) {
            Ok(bytes) => match std::str::from_utf8(&bytes).ok().and_then(|t| pyjson::loads(t).ok()) {
                Some(v) => return Some(v),
                None if attempt == 0 => std::thread::sleep(std::time::Duration::from_millis(50)),
                None => return None,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && attempt == 0 => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            Err(_) => return None,
        }
    }
    None
}

/// `_parse_dict` + `_hydrate_cfg`.
pub fn hydrate(data: &Value) -> Option<LopeCfg> {
    let obj = data.as_object()?;
    // data.get("version", 0) != VERSION → None (1 == True in Python, 1.0 too)
    let version_ok = match obj.get("version") {
        None => false,
        Some(Value::Bool(true)) => true,
        Some(Value::Number(n)) => n.as_f64() == Some(1.0),
        _ => false,
    };
    if !version_ok {
        return None;
    }
    let validators = obj.get("validators")?.as_array()?.clone();
    let primary = obj.get("primary")?.as_str()?.to_string();
    let timeout = obj.get("timeout").cloned().unwrap_or(int_value(DEFAULT_MODEL_CALL_TIMEOUT_SECONDS));
    py_int(&timeout)?;
    let parallel = match obj.get("parallel") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return None,
    };
    let providers = match obj.get("providers") {
        Some(Value::Array(a)) => a.clone(),
        _ => vec![],
    };
    let run_timeout = match obj.get("run_timeout") {
        None | Some(Value::Null) => None,
        Some(v) => match py_int(v) {
            Some(i) if i > 0 => Some(v.clone()),
            _ => return None,
        },
    };
    let allow_unbounded_run = match obj.get("allow_unbounded_run") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return None,
    };
    let get_pos = |k: &str, d: i64| -> Option<Value> {
        let v = obj.get(k).cloned().unwrap_or(int_value(d));
        match py_int(&v) {
            Some(i) if i > 0 => Some(v),
            _ => None,
        }
    };
    let max_calls = get_pos("max_calls", DEFAULT_MAX_EXTERNAL_CALLS)?;
    let max_input_bytes = get_pos("max_input_bytes", DEFAULT_MAX_INPUT_BYTES)?;
    let max_output_bytes = get_pos("max_output_bytes", DEFAULT_MAX_OUTPUT_BYTES)?;
    let max_chunks = get_pos("max_chunks", DEFAULT_MAX_CHUNKS)?;
    let request_policy = match obj.get("request_policy") {
        None => "auto".to_string(),
        Some(Value::String(s)) if matches!(s.as_str(), "auto" | "direct" | "chunk") => s.clone(),
        Some(_) => return None,
    };
    let mut learned = Vec::new();
    if let Some(Value::Object(raw)) = obj.get("learned_adapters") {
        for (name, entry) in raw {
            let Some(e) = entry.as_object() else { continue };
            let argv = e.get("argv_template").cloned().unwrap_or(Value::Array(vec![]));
            let Some(items) = argv.as_array() else {
                continue;
            };
            if !items.iter().all(Value::is_string) {
                continue;
            }
            let argv_template: Vec<String> = items.iter().map(|x| x.as_str().unwrap().to_string()).collect();
            // float(...) raising in Python aborts the whole load; mirror as None.
            let timestamp = py_float(e.get("timestamp").unwrap_or(&Value::from(0.0)))?;
            let confidence = py_float(e.get("confidence").unwrap_or(&Value::from(0.0)))?;
            learned.retain(|(n, _): &(String, LearnedAdapter)| n != name);
            learned.push((
                name.clone(),
                LearnedAdapter {
                    argv_template,
                    stdin_mode: e.get("stdin_mode").cloned().unwrap_or(Value::from("none")),
                    stdout_parser: e.get("stdout_parser").cloned().unwrap_or(Value::from("plaintext")),
                    timestamp,
                    source_cli: e.get("source_cli").cloned().unwrap_or(Value::from("")),
                    confidence,
                },
            ));
        }
    }
    Some(LopeCfg {
        validators,
        primary,
        timeout,
        parallel,
        providers,
        learned_adapters: learned,
        run_timeout,
        allow_unbounded_run,
        max_calls,
        max_input_bytes,
        max_output_bytes,
        request_policy,
        max_chunks,
    })
}

/// `load(path)`.
pub fn load(path: &Path) -> Option<LopeCfg> {
    hydrate(&safe_read(path)?)
}

/// CLI overrides for `load_layered` (layer 5); `None` = not given.
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub validators: Option<Vec<String>>,
    pub primary: Option<String>,
    pub timeout: Option<i64>,
    pub parallel: Option<bool>,
    pub run_timeout: Option<i64>,
    pub allow_unbounded_run: Option<bool>,
    pub max_calls: Option<i64>,
    pub max_input_bytes: Option<i64>,
    pub max_output_bytes: Option<i64>,
    pub request_policy: Option<String>,
    pub max_chunks: Option<i64>,
}

fn truthy(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

/// Python `int(s)` for env values (whitespace, sign, underscores).
pub fn py_int_str(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, body) = match t.as_bytes().first()? {
        b'+' => (false, &t[1..]),
        b'-' => (true, &t[1..]),
        _ => (false, t),
    };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    if !body.chars().all(|c| c.is_ascii_digit() || c == '_') {
        return None;
    }
    let v: i64 = body.replace('_', "").parse().ok()?;
    Some(if neg { -v } else { v })
}

/// `load_layered(cwd, env, cli_overrides)`: built-in < user global <
/// project < env < CLI, field by field.
pub fn load_layered(cwd: &Path, env: &dyn Fn(&str) -> Option<String>, cli: &CliOverrides) -> LopeCfg {
    let mut m = LopeCfg::defaults();
    if let Some(g) = load(&default_path()) {
        m = g;
    }
    if let Some(Value::Object(p)) = safe_read(&project_path(cwd)) {
        if !p.is_empty() {
            let version_ok = match p.get("version") {
                None => true,
                Some(Value::Bool(true)) => true,
                Some(Value::Number(n)) => n.as_f64() == Some(1.0),
                _ => false,
            };
            if version_ok {
                if let Some(Value::Array(v)) = p.get("validators") {
                    m.validators = v.iter().filter(|x| x.is_string()).cloned().collect();
                }
                if let Some(Value::String(s)) = p.get("primary") {
                    m.primary = s.clone();
                }
                if let Some(v) = p.get("timeout") {
                    if py_int(v).is_some() {
                        m.timeout = v.clone();
                    }
                }
                if let Some(Value::Bool(b)) = p.get("parallel") {
                    m.parallel = *b;
                }
                if let Some(Value::Array(a)) = p.get("providers") {
                    if !a.is_empty() {
                        m.providers = a.clone();
                    }
                }
                if let Some(v) = p.get("run_timeout") {
                    if py_int(v).is_some_and(|i| i > 0) {
                        m.run_timeout = Some(v.clone());
                    }
                }
                if let Some(Value::Bool(b)) = p.get("allow_unbounded_run") {
                    m.allow_unbounded_run = *b;
                }
                for (key, slot) in [
                    ("max_calls", &mut m.max_calls),
                    ("max_input_bytes", &mut m.max_input_bytes),
                    ("max_output_bytes", &mut m.max_output_bytes),
                    ("max_chunks", &mut m.max_chunks),
                ] {
                    if let Some(v) = p.get(key) {
                        if py_int(v).is_some_and(|i| i > 0) {
                            *slot = v.clone();
                        }
                    }
                }
                if let Some(Value::String(s)) = p.get("request_policy") {
                    if matches!(s.as_str(), "auto" | "direct" | "chunk") {
                        m.request_policy = s.clone();
                    }
                }
            }
        }
    }
    if let Some(v) = env("LOPE_VALIDATORS").filter(|v| !v.is_empty()) {
        m.validators = v.split(',').map(str::trim).filter(|s| !s.is_empty()).map(Value::from).collect();
    }
    if let Some(v) = env("LOPE_PRIMARY").filter(|v| !v.is_empty()) {
        m.primary = v;
    }
    if let Some(v) = env("LOPE_TIMEOUT").filter(|v| !v.is_empty()) {
        if let Some(i) = py_int_str(&v) {
            m.timeout = int_value(i);
        }
    }
    if let Some(v) = env("LOPE_PARALLEL") {
        m.parallel = truthy(&v);
    }
    if truthy(&env("LOPE_SEQUENTIAL").unwrap_or_default()) {
        m.parallel = false;
    }
    for (name, key) in [
        ("LOPE_RUN_TIMEOUT", "run_timeout"),
        ("LOPE_MAX_CALLS", "max_calls"),
        ("LOPE_MAX_INPUT_BYTES", "max_input_bytes"),
        ("LOPE_MAX_OUTPUT_BYTES", "max_output_bytes"),
        ("LOPE_MAX_CHUNKS", "max_chunks"),
    ] {
        if let Some(raw) = env(name).filter(|v| !v.is_empty()) {
            if let Some(i) = py_int_str(&raw).filter(|i| *i > 0) {
                match key {
                    "run_timeout" => m.run_timeout = Some(int_value(i)),
                    "max_calls" => m.max_calls = int_value(i),
                    "max_input_bytes" => m.max_input_bytes = int_value(i),
                    "max_output_bytes" => m.max_output_bytes = int_value(i),
                    _ => m.max_chunks = int_value(i),
                }
            }
        }
    }
    if truthy(&env("LOPE_ALLOW_UNBOUNDED_RUN").unwrap_or_default()) {
        m.allow_unbounded_run = true;
        m.run_timeout = None;
    }
    if let Some(p) = env("LOPE_REQUEST_POLICY") {
        if matches!(p.as_str(), "auto" | "direct" | "chunk") {
            m.request_policy = p;
        }
    }
    if let Some(v) = &cli.validators {
        m.validators = v.iter().cloned().map(Value::from).collect();
    }
    if let Some(v) = &cli.primary {
        m.primary = v.clone();
    }
    if let Some(v) = cli.timeout {
        m.timeout = int_value(v);
    }
    if let Some(v) = cli.parallel {
        m.parallel = v;
    }
    if let Some(v) = cli.run_timeout {
        m.run_timeout = Some(int_value(v));
    }
    if let Some(v) = cli.allow_unbounded_run {
        m.allow_unbounded_run = v;
    }
    if let Some(v) = cli.max_calls {
        m.max_calls = int_value(v);
    }
    if let Some(v) = cli.max_input_bytes {
        m.max_input_bytes = int_value(v);
    }
    if let Some(v) = cli.max_output_bytes {
        m.max_output_bytes = int_value(v);
    }
    if let Some(v) = &cli.request_policy {
        m.request_policy = v.clone();
    }
    if let Some(v) = cli.max_chunks {
        m.max_chunks = int_value(v);
    }
    if cli.allow_unbounded_run == Some(true) {
        m.run_timeout = None;
    } else if cli.run_timeout.is_some() {
        m.allow_unbounded_run = false;
    }
    m
}

/// The exact JSON object Python's `save()` writes.
pub fn to_saved_json(cfg: &LopeCfg) -> Value {
    let mut d = Map::new();
    d.insert("version".into(), int_value(VERSION));
    d.insert("validators".into(), Value::Array(cfg.validators.clone()));
    d.insert("primary".into(), Value::from(cfg.primary.clone()));
    d.insert("timeout".into(), cfg.timeout.clone());
    d.insert("parallel".into(), Value::Bool(cfg.parallel));
    d.insert("providers".into(), Value::Array(cfg.providers.clone()));
    if !cfg.learned_adapters.is_empty() {
        let mut la = Map::new();
        for (name, a) in &cfg.learned_adapters {
            la.insert(name.clone(), a.to_json());
        }
        d.insert("learned_adapters".into(), Value::Object(la));
    }
    if let Some(rt) = &cfg.run_timeout {
        d.insert("run_timeout".into(), rt.clone());
    }
    if cfg.allow_unbounded_run {
        d.insert("allow_unbounded_run".into(), Value::Bool(true));
    }
    for (key, v, default) in [
        ("max_calls", &cfg.max_calls, DEFAULT_MAX_EXTERNAL_CALLS),
        ("max_input_bytes", &cfg.max_input_bytes, DEFAULT_MAX_INPUT_BYTES),
        ("max_output_bytes", &cfg.max_output_bytes, DEFAULT_MAX_OUTPUT_BYTES),
    ] {
        if py_int(v) != Some(default) {
            d.insert(key.into(), v.clone());
        }
    }
    if cfg.request_policy != "auto" {
        d.insert("request_policy".into(), Value::from(cfg.request_policy.clone()));
    }
    if py_int(&cfg.max_chunks) != Some(DEFAULT_MAX_CHUNKS) {
        d.insert("max_chunks".into(), cfg.max_chunks.clone());
    }
    Value::Object(d)
}

/// `save(cfg, path)`: exclusive flock on `<path>.lock` (opened "a+"), write
/// `json.dump(data)` to a 0600 `tmpXXXXXXXX.tmp` in the same directory,
/// fsync, rename over the target.
pub fn save(cfg: &LopeCfg, path: &Path) -> std::io::Result<()> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let text = pyjson::dumps(&to_saved_json(cfg), &Opts::default());
    let mut lock_name = path.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock = std::fs::OpenOptions::new().create(true).append(true).read(true).open(PathBuf::from(lock_name))?;
    let fd = std::os::unix::io::AsRawFd::as_raw_fd(&lock);
    let locked = unsafe { libc::flock(fd, libc::LOCK_EX) } == 0;
    let result = (|| {
        let mut tmp = tempfile_in(parent)?;
        tmp.1.write_all(text.as_bytes())?;
        tmp.1.flush()?;
        tmp.1.sync_all()?;
        if let Err(e) = std::fs::rename(&tmp.0, path) {
            let _ = std::fs::remove_file(&tmp.0);
            return Err(e);
        }
        Ok(())
    })();
    if locked {
        unsafe { libc::flock(fd, libc::LOCK_UN) };
    }
    result
}

/// `tempfile.mkstemp(dir=..., suffix=".tmp")`: `tmp` + 8 random chars, 0600.
pub fn tempfile_in(dir: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    use std::os::unix::fs::OpenOptionsExt;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    for _ in 0..100 {
        let bytes = uuid::Uuid::new_v4().into_bytes();
        let name: String = bytes[..8].iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect();
        let p = dir.join(format!("tmp{name}.tmp"));
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&p) {
            Ok(f) => return Ok((p, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("could not create temporary file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bools_are_ints_floats_are_not() {
        let v = pyjson::loads(r#"{"version": 1, "validators": [], "primary": "", "timeout": true}"#).unwrap();
        assert_eq!(hydrate(&v).unwrap().timeout, Value::Bool(true));
        let v = pyjson::loads(r#"{"version": 1, "validators": [], "primary": "", "timeout": 9.0}"#).unwrap();
        assert!(hydrate(&v).is_none());
        let v = pyjson::loads(r#"{"version": 2, "validators": [], "primary": ""}"#).unwrap();
        assert!(hydrate(&v).is_none());
        let v = pyjson::loads(r#"{"version": 1.0, "validators": [], "primary": ""}"#).unwrap();
        assert!(hydrate(&v).is_some());
    }

    #[test]
    fn saved_shape_omits_defaults() {
        let mut c = LopeCfg::defaults();
        c.validators = vec![Value::from("a")];
        c.primary = "a".into();
        assert_eq!(
            pyjson::dumps(&to_saved_json(&c), &Opts::default()),
            r#"{"version": 1, "validators": ["a"], "primary": "a", "timeout": 960, "parallel": true, "providers": []}"#
        );
    }
}
