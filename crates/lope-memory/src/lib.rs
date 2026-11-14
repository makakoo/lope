//! `memory.py`: SQLite-backed cross-session memory store (`memory.db`, S4).
//!
//! Uses system `-lsqlite3` directly (the same library Python's `sqlite3` module
//! uses) with `PRAGMA journal_mode = WAL`, `PRAGMA synchronous = NORMAL`,
//! `sqlite3_busy_timeout(5000)`, and `PRAGMA user_version = 1`.

use libc::{c_char, c_double, c_int, c_void};
use lope_core::pyjson;
use lope_core::redaction::redact_text;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

pub const ENV_DISABLE: &str = "LOPE_MEMORY";
pub const ENV_DB_PATH: &str = "LOPE_MEMORY_DB";
pub const DEFAULT_DIR_NAME: &str = ".lope";
pub const DEFAULT_DB_NAME: &str = "memory.db";
pub const SCHEMA_VERSION: i64 = 1;

// ─── Minimal safe SQLite3 FFI wrapper ───────────────────────────────────────

#[repr(C)]
struct sqlite3(c_void);
#[repr(C)]
struct sqlite3_stmt(c_void);

const SQLITE_OK: c_int = 0;
const SQLITE_ROW: c_int = 100;
const SQLITE_DONE: c_int = 101;

const SQLITE_INTEGER: c_int = 1;
const SQLITE_FLOAT: c_int = 2;
const SQLITE_TEXT: c_int = 3;
const SQLITE_NULL: c_int = 5;

const SQLITE_TRANSIENT: isize = -1;

unsafe extern "C" {
    fn sqlite3_open(filename: *const c_char, pp_db: *mut *mut sqlite3) -> c_int;
    fn sqlite3_close(db: *mut sqlite3) -> c_int;
    fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
    fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_free(ptr: *mut c_void);
    fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    fn sqlite3_changes(db: *mut sqlite3) -> c_int;
    fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> i64;
    fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        z_sql: *const c_char,
        n_byte: c_int,
        pp_stmt: *mut *mut sqlite3_stmt,
        pz_tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut sqlite3_stmt,
        idx: c_int,
        val: *const c_char,
        n: c_int,
        destructor: isize,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, idx: c_int, val: i64) -> c_int;
    fn sqlite3_bind_double(stmt: *mut sqlite3_stmt, idx: c_int, val: c_double) -> c_int;
    fn sqlite3_bind_null(stmt: *mut sqlite3_stmt, idx: c_int) -> c_int;
    fn sqlite3_column_count(stmt: *mut sqlite3_stmt) -> c_int;
    fn sqlite3_column_name(stmt: *mut sqlite3_stmt, n: c_int) -> *const c_char;
    fn sqlite3_column_type(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_text(stmt: *mut sqlite3_stmt, i_col: c_int) -> *const u8;
    fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_int;
    fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, i_col: c_int) -> i64;
    fn sqlite3_column_double(stmt: *mut sqlite3_stmt, i_col: c_int) -> c_double;
}

#[derive(Debug, Clone)]
enum SqlParam {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
}

impl From<i64> for SqlParam {
    fn from(v: i64) -> Self {
        SqlParam::Int(v)
    }
}
impl From<f64> for SqlParam {
    fn from(v: f64) -> Self {
        SqlParam::Float(v)
    }
}
impl From<&str> for SqlParam {
    fn from(v: &str) -> Self {
        SqlParam::Text(v.to_string())
    }
}
impl From<String> for SqlParam {
    fn from(v: String) -> Self {
        SqlParam::Text(v)
    }
}
impl From<Option<&str>> for SqlParam {
    fn from(v: Option<&str>) -> Self {
        match v {
            Some(s) => SqlParam::Text(s.to_string()),
            None => SqlParam::Null,
        }
    }
}
impl From<Option<String>> for SqlParam {
    fn from(v: Option<String>) -> Self {
        match v {
            Some(s) => SqlParam::Text(s),
            None => SqlParam::Null,
        }
    }
}
impl From<Option<i64>> for SqlParam {
    fn from(v: Option<i64>) -> Self {
        match v {
            Some(i) => SqlParam::Int(i),
            None => SqlParam::Null,
        }
    }
}
impl From<Option<f64>> for SqlParam {
    fn from(v: Option<f64>) -> Self {
        match v {
            Some(f) => SqlParam::Float(f),
            None => SqlParam::Null,
        }
    }
}

struct SqliteConn {
    db: *mut sqlite3,
}

impl Drop for SqliteConn {
    fn drop(&mut self) {
        if !self.db.is_null() {
            unsafe {
                let _ = sqlite3_close(self.db);
            }
        }
    }
}

impl SqliteConn {
    fn open(path: &Path) -> Result<Self, String> {
        let cpath = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|e| format!("invalid db path: {e}"))?;
        let mut db: *mut sqlite3 = ptr::null_mut();
        let rc = unsafe { sqlite3_open(cpath.as_ptr(), &mut db) };
        if rc != SQLITE_OK {
            let msg = if db.is_null() {
                format!("sqlite3_open failed with code {rc}")
            } else {
                let s = unsafe { CStr::from_ptr(sqlite3_errmsg(db)).to_string_lossy().into_owned() };
                unsafe {
                    let _ = sqlite3_close(db);
                }
                s
            };
            return Err(msg);
        }
        unsafe {
            let _ = sqlite3_busy_timeout(db, 5000);
        }
        let conn = SqliteConn { db };
        conn.exec_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        Ok(conn)
    }

    fn err_msg(&self) -> String {
        unsafe { CStr::from_ptr(sqlite3_errmsg(self.db)).to_string_lossy().into_owned() }
    }

    fn exec_batch(&self, sql: &str) -> Result<(), String> {
        let csql = CString::new(sql).map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut errmsg: *mut c_char = ptr::null_mut();
            let rc =
                unsafe { sqlite3_exec(self.db, csql.as_ptr(), None, ptr::null_mut(), &mut errmsg) };
            if rc == SQLITE_OK {
                return Ok(());
            }
            let msg = if !errmsg.is_null() {
                let s = unsafe { CStr::from_ptr(errmsg).to_string_lossy().into_owned() };
                unsafe { sqlite3_free(errmsg as *mut c_void) };
                s
            } else {
                self.err_msg()
            };
            if msg.contains("locked") && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            return Err(msg);
        }
    }

    fn execute(&self, sql: &str, params: &[SqlParam]) -> Result<(i64, i64), String> {
        let csql = CString::new(sql).map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut stmt: *mut sqlite3_stmt = ptr::null_mut();
            let rc = unsafe {
                sqlite3_prepare_v2(self.db, csql.as_ptr(), -1, &mut stmt, ptr::null_mut())
            };
            if rc != SQLITE_OK {
                let msg = self.err_msg();
                if msg.contains("locked") && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                return Err(msg);
            }
            struct StmtGuard(*mut sqlite3_stmt);
            impl Drop for StmtGuard {
                fn drop(&mut self) {
                    unsafe {
                        let _ = sqlite3_finalize(self.0);
                    }
                }
            }
            let _guard = StmtGuard(stmt);
            bind_params(stmt, params, self)?;
            let step_rc = unsafe { sqlite3_step(stmt) };
            if step_rc != SQLITE_DONE && step_rc != SQLITE_ROW {
                let msg = self.err_msg();
                if msg.contains("locked") && std::time::Instant::now() < deadline {
                    drop(_guard);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                }
                return Err(msg);
            }
            let changes = unsafe { sqlite3_changes(self.db) } as i64;
            let last_id = unsafe { sqlite3_last_insert_rowid(self.db) };
            return Ok((changes, last_id));
        }
    }

    fn query(&self, sql: &str, params: &[SqlParam]) -> Result<Vec<Map<String, Value>>, String> {
        let csql = CString::new(sql).map_err(|e| e.to_string())?;
        let mut stmt: *mut sqlite3_stmt = ptr::null_mut();
        let rc = unsafe { sqlite3_prepare_v2(self.db, csql.as_ptr(), -1, &mut stmt, ptr::null_mut()) };
        if rc != SQLITE_OK {
            return Err(self.err_msg());
        }
        struct StmtGuard(*mut sqlite3_stmt);
        impl Drop for StmtGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = sqlite3_finalize(self.0);
                }
            }
        }
        let _guard = StmtGuard(stmt);
        bind_params(stmt, params, self)?;

        let col_count = unsafe { sqlite3_column_count(stmt) };
        let mut col_names = Vec::with_capacity(col_count as usize);
        for i in 0..col_count {
            let name_ptr = unsafe { sqlite3_column_name(stmt, i) };
            let name = if name_ptr.is_null() {
                format!("col_{i}")
            } else {
                unsafe { CStr::from_ptr(name_ptr).to_string_lossy().into_owned() }
            };
            col_names.push(name);
        }

        let mut rows = Vec::new();
        loop {
            let step_rc = unsafe { sqlite3_step(stmt) };
            if step_rc == SQLITE_DONE {
                break;
            }
            if step_rc != SQLITE_ROW {
                return Err(self.err_msg());
            }
            let mut row = Map::new();
            for (i, col_name) in col_names.iter().enumerate() {
                let idx = i as c_int;
                let ctype = unsafe { sqlite3_column_type(stmt, idx) };
                let val = match ctype {
                    SQLITE_NULL => Value::Null,
                    SQLITE_INTEGER => Value::from(unsafe { sqlite3_column_int64(stmt, idx) }),
                    SQLITE_FLOAT => {
                        let d = unsafe { sqlite3_column_double(stmt, idx) };
                        serde_json::Number::from_f64(d).map(Value::Number).unwrap_or(Value::Null)
                    }
                    SQLITE_TEXT | _ => {
                        let ptr = unsafe { sqlite3_column_text(stmt, idx) };
                        let len = unsafe { sqlite3_column_bytes(stmt, idx) };
                        if ptr.is_null() || len <= 0 {
                            Value::String(String::new())
                        } else {
                            let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
                            Value::String(String::from_utf8_lossy(slice).into_owned())
                        }
                    }
                };
                row.insert(col_name.clone(), val);
            }
            rows.push(row);
        }
        Ok(rows)
    }
}

fn bind_params(stmt: *mut sqlite3_stmt, params: &[SqlParam], conn: &SqliteConn) -> Result<(), String> {
    for (i, p) in params.iter().enumerate() {
        let idx = (i + 1) as c_int;
        let rc = match p {
            SqlParam::Null => unsafe { sqlite3_bind_null(stmt, idx) },
            SqlParam::Int(v) => unsafe { sqlite3_bind_int64(stmt, idx, *v) },
            SqlParam::Float(v) => unsafe { sqlite3_bind_double(stmt, idx, *v) },
            SqlParam::Text(s) => unsafe {
                sqlite3_bind_text(
                    stmt,
                    idx,
                    s.as_ptr() as *const c_char,
                    s.len() as c_int,
                    SQLITE_TRANSIENT,
                )
            },
        };
        if rc != SQLITE_OK {
            return Err(conn.err_msg());
        }
    }
    Ok(())
}

// ─── Config & time helpers ──────────────────────────────────────────────────

pub fn is_memory_disabled() -> bool {
    let Ok(raw) = std::env::var(ENV_DISABLE) else {
        return false;
    };
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "off" | "no" | "0" | "false" | "disabled"
    )
}

pub fn default_db_path() -> PathBuf {
    if let Ok(exp) = std::env::var(ENV_DB_PATH) {
        let s = exp.trim();
        if !s.is_empty() {
            if let Some(rest) = s.strip_prefix("~/") {
                return lope_core::paths::home_dir().join(rest);
            }
            return PathBuf::from(s);
        }
    }
    lope_core::paths::home_dir()
        .join(DEFAULT_DIR_NAME)
        .join(DEFAULT_DB_NAME)
}

fn utc_iso_from_epoch(secs: i64) -> String {
    let mut t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        libc::gmtime_r(&mut t, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

pub fn now_iso() -> String {
    utc_iso_from_epoch(lope_core::time::now() as i64)
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// ─── Records ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct FindingRecord {
    pub id: Option<i64>,
    pub hash: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub end_line: Option<i64>,
    pub severity: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub consensus_score: f64,
    pub consensus_level: String,
    pub agreement_count: i64,
    pub total_validators: i64,
    pub detected_by: Vec<String>,
    pub evidence: Map<String, Value>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub seen_count: i64,
}

impl FindingRecord {
    pub fn to_dict(&self) -> Value {
        json!({
            "id": self.id,
            "hash": self.hash,
            "message": self.message,
            "file": self.file,
            "line": self.line,
            "end_line": self.end_line,
            "severity": self.severity,
            "category": self.category,
            "confidence": round3(self.confidence),
            "consensus_score": round3(self.consensus_score),
            "consensus_level": self.consensus_level,
            "agreement_count": self.agreement_count,
            "total_validators": self.total_validators,
            "detected_by": self.detected_by,
            "evidence": self.evidence,
            "first_seen_at": self.first_seen_at,
            "last_seen_at": self.last_seen_at,
            "seen_count": self.seen_count,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ConsensusFindingInput {
    pub hash: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub end_line: Option<i64>,
    pub severity: String,
    pub category: Option<String>,
    pub confidence_max: f64,
    pub consensus_score: f64,
    pub consensus_level: String,
    pub agreement_count: i64,
    pub total_validators: i64,
    pub detected_by: Vec<String>,
    pub evidence: Map<String, Value>,
}

fn record_from_consensus(f: &ConsensusFindingInput, now: &str) -> FindingRecord {
    let detected: Vec<String> = f
        .detected_by
        .iter()
        .map(|n| redact_text(n).trim().to_string())
        .collect();
    let mut evidence = Map::new();
    for (k, v) in &f.evidence {
        let vs = v.as_str().unwrap_or("");
        evidence.insert(
            redact_text(k).trim().to_string(),
            Value::String(redact_text(vs).trim().to_string()),
        );
    }
    let file_val = f
        .file
        .as_deref()
        .map(|s| redact_text(s).trim().to_string())
        .filter(|s| !s.is_empty());
    let cat_val = f
        .category
        .as_deref()
        .map(|s| redact_text(s).trim().to_string())
        .filter(|s| !s.is_empty());
    let sev = if f.severity.is_empty() {
        "info".to_string()
    } else {
        f.severity.clone()
    };
    FindingRecord {
        id: None,
        hash: f.hash.clone(),
        message: redact_text(&f.message).trim().to_string(),
        file: file_val,
        line: f.line,
        end_line: f.end_line,
        severity: sev,
        category: cat_val,
        confidence: f.confidence_max,
        consensus_score: f.consensus_score,
        consensus_level: f.consensus_level.clone(),
        agreement_count: f.agreement_count,
        total_validators: f.total_validators,
        detected_by: detected,
        evidence,
        first_seen_at: now.to_string(),
        last_seen_at: now.to_string(),
        seen_count: 1,
    }
}

fn level_rank(s: &str) -> i32 {
    match s {
        "confirmed" => 4,
        "likely" => 3,
        "needs-verification" => 2,
        "unverified" => 1,
        _ => 0,
    }
}

fn max_consensus_level<'a>(a: &'a str, b: &'a str) -> &'a str {
    if level_rank(b) > level_rank(a) {
        b
    } else {
        a
    }
}

fn merge_string_lists(existing: &[String], incoming: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for v in existing.iter().chain(incoming.iter()) {
        if !v.is_empty() && !out.contains(v) {
            out.push(v.clone());
        }
    }
    out
}

fn row_to_finding(r: &Map<String, Value>) -> FindingRecord {
    let detected_by: Vec<String> = r
        .get("detected_by_json")
        .and_then(Value::as_str)
        .and_then(|s| pyjson::loads(s).ok())
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.into_iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let evidence: Map<String, Value> = r
        .get("evidence_json")
        .and_then(Value::as_str)
        .and_then(|s| pyjson::loads(s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    FindingRecord {
        id: r.get("id").and_then(Value::as_i64),
        hash: r
            .get("hash")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        message: r
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        file: r.get("file").and_then(Value::as_str).map(String::from),
        line: r.get("line").and_then(Value::as_i64),
        end_line: r.get("end_line").and_then(Value::as_i64),
        severity: r
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("info")
            .to_string(),
        category: r.get("category").and_then(Value::as_str).map(String::from),
        confidence: r.get("confidence").and_then(Value::as_f64).unwrap_or(0.0),
        consensus_score: r
            .get("consensus_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        consensus_level: r
            .get("consensus_level")
            .and_then(Value::as_str)
            .unwrap_or("unverified")
            .to_string(),
        agreement_count: r.get("agreement_count").and_then(Value::as_i64).unwrap_or(0),
        total_validators: r
            .get("total_validators")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        detected_by,
        evidence,
        first_seen_at: r
            .get("first_seen_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        last_seen_at: r
            .get("last_seen_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        seen_count: r.get("seen_count").and_then(Value::as_i64).unwrap_or(1),
    }
}

// ─── LopeMemory ─────────────────────────────────────────────────────────────

const SCHEMA_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS findings (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        hash            TEXT UNIQUE NOT NULL,
        message         TEXT NOT NULL,
        file            TEXT,
        line            INTEGER,
        end_line        INTEGER,
        severity        TEXT NOT NULL DEFAULT 'info',
        category        TEXT,
        confidence      REAL NOT NULL DEFAULT 0.0,
        consensus_score REAL NOT NULL DEFAULT 0.0,
        consensus_level TEXT NOT NULL DEFAULT 'unverified',
        agreement_count INTEGER NOT NULL DEFAULT 0,
        total_validators INTEGER NOT NULL DEFAULT 0,
        detected_by_json TEXT NOT NULL DEFAULT '[]',
        evidence_json    TEXT NOT NULL DEFAULT '{}',
        first_seen_at    TEXT NOT NULL,
        last_seen_at     TEXT NOT NULL,
        seen_count       INTEGER NOT NULL DEFAULT 1
    );

    CREATE TABLE IF NOT EXISTS review_sessions (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        task            TEXT NOT NULL DEFAULT '',
        focus           TEXT NOT NULL DEFAULT '',
        target_path     TEXT NOT NULL DEFAULT '',
        validators_json TEXT NOT NULL DEFAULT '[]',
        raw_count       INTEGER NOT NULL DEFAULT 0,
        merged_count    INTEGER NOT NULL DEFAULT 0,
        created_at      TEXT NOT NULL,
        duration_ms     INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS session_findings (
        session_id    INTEGER NOT NULL,
        finding_hash  TEXT NOT NULL,
        PRIMARY KEY (session_id, finding_hash)
    );

    CREATE TABLE IF NOT EXISTS gate_sessions (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        task            TEXT NOT NULL DEFAULT '',
        mode            TEXT NOT NULL DEFAULT '',
        baseline_path   TEXT NOT NULL DEFAULT '',
        created_at      TEXT NOT NULL,
        passed          INTEGER NOT NULL DEFAULT 0,
        gate_count      INTEGER NOT NULL DEFAULT 0,
        failed_count    INTEGER NOT NULL DEFAULT 0,
        payload_json    TEXT NOT NULL DEFAULT '{}'
    );

    CREATE INDEX IF NOT EXISTS idx_findings_hash         ON findings(hash);
    CREATE INDEX IF NOT EXISTS idx_findings_file         ON findings(file);
    CREATE INDEX IF NOT EXISTS idx_findings_score        ON findings(consensus_score DESC);
    CREATE INDEX IF NOT EXISTS idx_findings_last_seen    ON findings(last_seen_at DESC);
    CREATE INDEX IF NOT EXISTS idx_session_findings_hash ON session_findings(finding_hash);
    CREATE INDEX IF NOT EXISTS idx_gate_sessions_created ON gate_sessions(created_at DESC);
"#;

const MIGRATION_V1_SQL: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS verdict_events (
        id                      INTEGER PRIMARY KEY AUTOINCREMENT,
        schema_version          TEXT    NOT NULL DEFAULT '1.0.0',
        run_id                  TEXT    NOT NULL,
        gate_id                 TEXT,
        validator_name          TEXT    NOT NULL,
        validator_version       TEXT,
        prompt_template_version TEXT    NOT NULL,
        rendered_prompt         TEXT    NOT NULL,
        task_spec_hash          TEXT    NOT NULL,
        validation_input_ref    TEXT,
        raw_response            TEXT    NOT NULL,
        parser_version          TEXT    NOT NULL,
        initial_parse_status    TEXT    NOT NULL,
        parse_error_category    TEXT,
        repair_attempted        INTEGER NOT NULL DEFAULT 0,
        repair_status           TEXT,
        repaired_response       TEXT,
        final_verdict           TEXT    NOT NULL,
        candidate_output_ref    TEXT,
        latency_s               REAL,
        exit_status             INTEGER,
        timed_out               INTEGER NOT NULL DEFAULT 0,
        created_at              TEXT    NOT NULL
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_verdict_events_run ON verdict_events(run_id)",
    "CREATE INDEX IF NOT EXISTS idx_verdict_events_validator ON verdict_events(validator_name, created_at DESC)",
    "CREATE INDEX IF NOT EXISTS idx_verdict_events_category ON verdict_events(parse_error_category)",
    "CREATE INDEX IF NOT EXISTS idx_verdict_events_created ON verdict_events(created_at DESC)",
];

pub struct LopeMemory {
    pub db_path: PathBuf,
}

pub fn open_memory() -> Result<Option<LopeMemory>, String> {
    if is_memory_disabled() {
        return Ok(None);
    }
    LopeMemory::open(&default_db_path()).map(Some)
}

impl LopeMemory {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mem = LopeMemory {
            db_path: db_path.to_path_buf(),
        };
        let conn = mem.connect()?;
        conn.exec_batch(SCHEMA_SQL)?;
        mem.migrate(&conn)?;
        Ok(mem)
    }

    fn connect(&self) -> Result<SqliteConn, String> {
        SqliteConn::open(&self.db_path)
    }

    fn migrate(&self, conn: &SqliteConn) -> Result<i64, String> {
        let rows = conn.query("PRAGMA user_version", &[])?;
        let current = rows
            .first()
            .and_then(|r| r.values().next())
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if current > SCHEMA_VERSION {
            return Err(format!(
                "{} was created by a newer Lope (schema v{current}; this build understands v{SCHEMA_VERSION}). Upgrade Lope, or point LOPE_MEMORY_DB at a different file.",
                self.db_path.display()
            ));
        }
        if current == SCHEMA_VERSION {
            return Ok(current);
        }

        conn.exec_batch("BEGIN IMMEDIATE")?;
        let step = (|| -> Result<(), String> {
            Self::reject_incompatible_tables(conn)?;
            for v in (current + 1)..=SCHEMA_VERSION {
                if v == 1 {
                    for stmt in MIGRATION_V1_SQL {
                        conn.exec_batch(stmt)?;
                    }
                }
            }
            Self::validate_schema(conn)?;
            conn.exec_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))?;
            Ok(())
        })();
        match step {
            Ok(()) => {
                conn.exec_batch("COMMIT")?;
                Ok(SCHEMA_VERSION)
            }
            Err(e) => {
                let _ = conn.exec_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    fn reject_incompatible_tables(conn: &SqliteConn) -> Result<(), String> {
        let exists = conn.query(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='verdict_events'",
            &[],
        )?;
        if exists.is_empty() {
            return Ok(());
        }
        let cols: HashSet<String> = conn
            .query("PRAGMA table_info(verdict_events)", &[])?
            .into_iter()
            .filter_map(|r| r.get("name").and_then(Value::as_str).map(String::from))
            .collect();
        let mut absent: Vec<&str> = ["run_id", "validator_name", "final_verdict", "created_at"]
            .into_iter()
            .filter(|c| !cols.contains(*c))
            .collect();
        if !absent.is_empty() {
            absent.sort();
            return Err(format!(
                "verdict_events already exists but is missing column(s) {absent:?} -- refusing to migrate over an incompatible table. Rename or drop it, then re-run."
            ));
        }
        Ok(())
    }

    fn validate_schema(conn: &SqliteConn) -> Result<(), String> {
        let names: HashSet<String> = conn
            .query("SELECT name FROM sqlite_master WHERE type = 'table'", &[])?
            .into_iter()
            .filter_map(|r| r.get("name").and_then(Value::as_str).map(String::from))
            .collect();
        let required = [
            "findings",
            "gate_sessions",
            "review_sessions",
            "session_findings",
            "verdict_events",
        ];
        let mut missing: Vec<&str> = required
            .into_iter()
            .filter(|t| !names.contains(*t))
            .collect();
        if !missing.is_empty() {
            missing.sort();
            return Err(format!(
                "migration validation failed: missing table(s) {missing:?}"
            ));
        }
        for t in required {
            let _ = conn.query(&format!("SELECT 1 FROM {t} LIMIT 1"), &[])?;
        }
        Ok(())
    }

    pub fn store_review_session(
        &self,
        task: &str,
        focus: &str,
        target_path: &str,
        validators: &[String],
        findings: &[ConsensusFindingInput],
        duration_ms: i64,
    ) -> Result<(i64, Vec<FindingRecord>), String> {
        let now = now_iso();
        let records: Vec<FindingRecord> = findings
            .iter()
            .map(|f| record_from_consensus(f, &now))
            .collect();
        let clean_validators: Vec<Value> = validators
            .iter()
            .map(|v| Value::String(redact_text(v).trim().to_string()))
            .collect();
        let validators_json =
            pyjson::dumps(&Value::Array(clean_validators), &pyjson::Opts::default());

        let conn = self.connect()?;
        let (_, session_id) = conn.execute(
            "INSERT INTO review_sessions (task, focus, target_path, validators_json, raw_count, merged_count, created_at, duration_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            &[
                SqlParam::from(redact_text(task).trim()),
                SqlParam::from(redact_text(focus).trim()),
                SqlParam::from(redact_text(target_path).trim()),
                SqlParam::from(validators_json),
                SqlParam::from(findings.len() as i64),
                SqlParam::from(findings.len() as i64),
                SqlParam::from(now.as_str()),
                SqlParam::from(duration_ms),
            ],
        )?;

        let mut stored = Vec::with_capacity(records.len());
        for rec in records {
            let up = self.upsert_finding(&conn, rec, &now)?;
            if !up.hash.is_empty() {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO session_findings (session_id, finding_hash) VALUES (?, ?)",
                    &[SqlParam::from(session_id), SqlParam::from(up.hash.as_str())],
                )?;
            }
            stored.push(up);
        }
        Ok((session_id, stored))
    }

    fn upsert_finding(
        &self,
        conn: &SqliteConn,
        mut record: FindingRecord,
        now: &str,
    ) -> Result<FindingRecord, String> {
        let existing_rows = conn.query(
            "SELECT * FROM findings WHERE hash = ?",
            &[SqlParam::from(record.hash.as_str())],
        )?;
        let Some(existing) = existing_rows.first() else {
            let det_json = pyjson::dumps(
                &Value::Array(
                    record
                        .detected_by
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
                &pyjson::Opts::default(),
            );
            let ev_json = pyjson::dumps(
                &Value::Object(record.evidence.clone()),
                &pyjson::Opts::default(),
            );
            let (_, row_id) = conn.execute(
                "INSERT INTO findings (hash, message, file, line, end_line, severity, category, confidence, consensus_score, consensus_level, agreement_count, total_validators, detected_by_json, evidence_json, first_seen_at, last_seen_at, seen_count) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                &[
                    SqlParam::from(record.hash.as_str()),
                    SqlParam::from(record.message.as_str()),
                    SqlParam::from(record.file.as_deref()),
                    SqlParam::from(record.line),
                    SqlParam::from(record.end_line),
                    SqlParam::from(record.severity.as_str()),
                    SqlParam::from(record.category.as_deref()),
                    SqlParam::from(record.confidence),
                    SqlParam::from(record.consensus_score),
                    SqlParam::from(record.consensus_level.as_str()),
                    SqlParam::from(record.agreement_count),
                    SqlParam::from(record.total_validators),
                    SqlParam::from(det_json),
                    SqlParam::from(ev_json),
                    SqlParam::from(record.first_seen_at.as_str()),
                    SqlParam::from(record.last_seen_at.as_str()),
                    SqlParam::from(record.seen_count),
                ],
            )?;
            record.id = Some(row_id);
            return Ok(record);
        };

        let ex = row_to_finding(existing);
        let merged_level =
            max_consensus_level(&ex.consensus_level, &record.consensus_level).to_string();
        let merged_detected = merge_string_lists(&ex.detected_by, &record.detected_by);
        let mut merged_evidence = ex.evidence.clone();
        for (k, v) in &record.evidence {
            merged_evidence.insert(k.clone(), v.clone());
        }
        let new_seen = ex.seen_count + 1;
        let new_confidence = ex.confidence.max(record.confidence);
        let new_score = ex.consensus_score.max(record.consensus_score);

        let det_json = pyjson::dumps(
            &Value::Array(merged_detected.iter().cloned().map(Value::String).collect()),
            &pyjson::Opts::default(),
        );
        let ev_json = pyjson::dumps(
            &Value::Object(merged_evidence.clone()),
            &pyjson::Opts::default(),
        );

        conn.execute(
            "UPDATE findings SET message = ?, file = ?, line = ?, end_line = ?, severity = ?, category = ?, confidence = ?, consensus_score = ?, consensus_level = ?, agreement_count = ?, total_validators = ?, detected_by_json = ?, evidence_json = ?, last_seen_at = ?, seen_count = ? WHERE hash = ?",
            &[
                SqlParam::from(record.message.as_str()),
                SqlParam::from(record.file.as_deref()),
                SqlParam::from(record.line),
                SqlParam::from(record.end_line),
                SqlParam::from(record.severity.as_str()),
                SqlParam::from(record.category.as_deref()),
                SqlParam::from(new_confidence),
                SqlParam::from(new_score),
                SqlParam::from(merged_level.as_str()),
                SqlParam::from(record.agreement_count),
                SqlParam::from(record.total_validators),
                SqlParam::from(det_json),
                SqlParam::from(ev_json),
                SqlParam::from(now),
                SqlParam::from(new_seen),
                SqlParam::from(record.hash.as_str()),
            ],
        )?;

        record.id = ex.id;
        record.first_seen_at = ex.first_seen_at;
        record.last_seen_at = now.to_string();
        record.seen_count = new_seen;
        record.confidence = new_confidence;
        record.consensus_score = new_score;
        record.consensus_level = merged_level;
        record.detected_by = merged_detected;
        record.evidence = merged_evidence;
        Ok(record)
    }

    pub fn store_gate_session(
        &self,
        task: &str,
        mode: &str,
        baseline_path: &str,
        passed: bool,
        gate_count: i64,
        failed_count: i64,
        payload: &Value,
    ) -> Result<i64, String> {
        let now = now_iso();
        let sort_opts = pyjson::Opts {
            indent: None,
            sort_keys: true,
            separators: None,
            ensure_ascii: true,
        };
        let dumped = pyjson::dumps(payload, &sort_opts);
        let redacted = redact_text(&dumped);
        let clean_val = pyjson::loads(&redacted).unwrap_or_else(|_| json!({}));
        let payload_json = pyjson::dumps(&clean_val, &sort_opts);

        let conn = self.connect()?;
        let (_, row_id) = conn.execute(
            "INSERT INTO gate_sessions (task, mode, baseline_path, created_at, passed, gate_count, failed_count, payload_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            &[
                SqlParam::from(redact_text(task).trim()),
                SqlParam::from(redact_text(mode).trim()),
                SqlParam::from(redact_text(baseline_path).trim()),
                SqlParam::from(now),
                SqlParam::from(if passed { 1i64 } else { 0i64 }),
                SqlParam::from(gate_count),
                SqlParam::from(failed_count),
                SqlParam::from(payload_json),
            ],
        )?;
        Ok(row_id)
    }

    pub fn gate_sessions(&self, limit: i64) -> Result<Vec<Value>, String> {
        let conn = self.connect()?;
        let rows = conn.query(
            "SELECT * FROM gate_sessions ORDER BY created_at DESC LIMIT ?",
            &[SqlParam::from(limit)],
        )?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let p_str = r
                .get("payload_json")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let payload = pyjson::loads(p_str).unwrap_or_else(|_| json!({}));
            out.push(json!({
                "id": r.get("id").and_then(Value::as_i64).unwrap_or(0),
                "task": r.get("task").and_then(Value::as_str).unwrap_or(""),
                "mode": r.get("mode").and_then(Value::as_str).unwrap_or(""),
                "baseline_path": r.get("baseline_path").and_then(Value::as_str).unwrap_or(""),
                "created_at": r.get("created_at").and_then(Value::as_str).unwrap_or(""),
                "passed": r.get("passed").and_then(Value::as_i64).unwrap_or(0) != 0,
                "gate_count": r.get("gate_count").and_then(Value::as_i64).unwrap_or(0),
                "failed_count": r.get("failed_count").and_then(Value::as_i64).unwrap_or(0),
                "payload": payload,
            }));
        }
        Ok(out)
    }

    pub fn search_findings(
        &self,
        query: &str,
        min_score: f64,
        limit: i64,
    ) -> Result<Vec<FindingRecord>, String> {
        let trimmed = query.trim();
        let like = if query.is_empty() {
            "%".to_string()
        } else {
            format!("%{trimmed}%")
        };
        let conn = self.connect()?;
        let rows = conn.query(
            "SELECT * FROM findings WHERE message LIKE ? AND consensus_score >= ? ORDER BY consensus_score DESC, seen_count DESC, last_seen_at DESC LIMIT ?",
            &[
                SqlParam::from(like),
                SqlParam::from(min_score),
                SqlParam::from(limit),
            ],
        )?;
        Ok(rows.iter().map(row_to_finding).collect())
    }

    pub fn findings_for_file(&self, file: &str, limit: i64) -> Result<Vec<FindingRecord>, String> {
        let conn = self.connect()?;
        let rows = conn.query(
            "SELECT * FROM findings WHERE file = ? ORDER BY consensus_score DESC, last_seen_at DESC LIMIT ?",
            &[SqlParam::from(file), SqlParam::from(limit)],
        )?;
        Ok(rows.iter().map(row_to_finding).collect())
    }

    pub fn hotspots(&self, days: i64, limit: i64) -> Result<Vec<Value>, String> {
        let cutoff_epoch = (lope_core::time::now() as i64) - days * 86400;
        let cutoff = utc_iso_from_epoch(cutoff_epoch);
        let conn = self.connect()?;
        let rows = conn.query(
            "SELECT file, COUNT(*) AS finding_count, SUM(seen_count) AS detection_count, AVG(consensus_score) AS avg_score, MAX(last_seen_at) AS last_seen_at FROM findings WHERE file IS NOT NULL AND last_seen_at >= ? GROUP BY file ORDER BY finding_count DESC, detection_count DESC LIMIT ?",
            &[SqlParam::from(cutoff), SqlParam::from(limit)],
        )?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let avg = r.get("avg_score").and_then(Value::as_f64).unwrap_or(0.0);
            out.push(json!({
                "file": r.get("file").and_then(Value::as_str).unwrap_or(""),
                "finding_count": r.get("finding_count").and_then(Value::as_i64).unwrap_or(0),
                "detection_count": r.get("detection_count").and_then(Value::as_i64).unwrap_or(0),
                "avg_score": round3(avg),
                "last_seen_at": r.get("last_seen_at").and_then(Value::as_str),
            }));
        }
        Ok(out)
    }

    pub fn stats(&self) -> Result<Value, String> {
        let conn = self.connect()?;
        let scalar = |sql: &str| -> Result<i64, String> {
            let r = conn.query(sql, &[])?;
            Ok(r.first()
                .and_then(|m| m.values().next())
                .and_then(Value::as_i64)
                .unwrap_or(0))
        };
        let total = scalar("SELECT COUNT(*) FROM findings")?;
        let sessions = scalar("SELECT COUNT(*) FROM review_sessions")?;
        let gate_sessions = scalar("SELECT COUNT(*) FROM gate_sessions")?;
        let confirmed =
            scalar("SELECT COUNT(*) FROM findings WHERE consensus_level = 'confirmed'")?;
        let recurring = scalar("SELECT COUNT(*) FROM findings WHERE seen_count > 1")?;
        let files = scalar("SELECT COUNT(DISTINCT file) FROM findings WHERE file IS NOT NULL")?;
        let last_rows = conn.query(
            "SELECT created_at FROM review_sessions ORDER BY created_at DESC LIMIT 1",
            &[],
        )?;
        let last_session_at = last_rows
            .first()
            .and_then(|m| m.get("created_at"))
            .and_then(Value::as_str)
            .map(|s| Value::String(s.to_string()))
            .unwrap_or(Value::Null);

        Ok(json!({
            "db_path": self.db_path.display().to_string(),
            "total_findings": total,
            "total_sessions": sessions,
            "total_gate_sessions": gate_sessions,
            "confirmed_findings": confirmed,
            "recurring_findings": recurring,
            "flagged_files": files,
            "last_session_at": last_session_at,
        }))
    }

    pub fn forget(&self, hash: Option<&str>, file: Option<&str>) -> Result<i64, String> {
        let h = hash.filter(|s| !s.is_empty());
        let f = file.filter(|s| !s.is_empty());
        if h.is_none() && f.is_none() {
            return Err("forget() requires hash or file".into());
        }
        let conn = self.connect()?;
        if let Some(hash_val) = h {
            conn.execute(
                "DELETE FROM session_findings WHERE finding_hash = ?",
                &[SqlParam::from(hash_val)],
            )?;
            let (changes, _) = conn.execute(
                "DELETE FROM findings WHERE hash = ?",
                &[SqlParam::from(hash_val)],
            )?;
            Ok(changes)
        } else {
            let file_val = f.unwrap();
            conn.execute(
                "DELETE FROM session_findings WHERE finding_hash IN (SELECT hash FROM findings WHERE file = ?)",
                &[SqlParam::from(file_val)],
            )?;
            let (changes, _) = conn.execute(
                "DELETE FROM findings WHERE file = ?",
                &[SqlParam::from(file_val)],
            )?;
            Ok(changes)
        }
    }

    pub fn clear(&self) -> Result<(), String> {
        let conn = self.connect()?;
        conn.exec_batch(
            "DELETE FROM session_findings; DELETE FROM gate_sessions; DELETE FROM review_sessions; DELETE FROM findings;",
        )
    }

    pub fn user_version(&self) -> Result<i64, String> {
        let conn = self.connect()?;
        let rows = conn.query("PRAGMA user_version", &[])?;
        Ok(rows
            .first()
            .and_then(|r| r.values().next())
            .and_then(Value::as_i64)
            .unwrap_or(0))
    }

    pub fn store_verdict_event(&self, ev: &VerdictEventInput) -> Result<i64, String> {
        let now = now_iso();
        let redacted_prompt = redact_text(&ev.rendered_prompt);
        let task_hash = lope_core::sha256_hex(redacted_prompt.as_bytes());
        let conn = self.connect()?;
        let (_, row_id) = conn.execute(
            "INSERT INTO verdict_events (schema_version, run_id, gate_id, validator_name, validator_version, prompt_template_version, rendered_prompt, task_spec_hash, validation_input_ref, raw_response, parser_version, initial_parse_status, parse_error_category, repair_attempted, repair_status, repaired_response, final_verdict, candidate_output_ref, latency_s, exit_status, timed_out, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            &[
                SqlParam::from("1.0.0"),
                SqlParam::from(ev.run_id.as_str()),
                SqlParam::from(ev.gate_id.as_deref()),
                SqlParam::from(ev.validator_name.as_str()),
                SqlParam::from(ev.validator_version.as_deref()),
                SqlParam::from("executor/_build_validation_prompt/1"),
                SqlParam::from(redacted_prompt),
                SqlParam::from(task_hash),
                SqlParam::from(ev.validation_input_ref.as_deref()),
                SqlParam::from(redact_text(&ev.raw_response)),
                SqlParam::from("parse_verdict_block/1"),
                SqlParam::from(ev.initial_parse_status.as_str()),
                SqlParam::from(ev.parse_error_category.as_deref()),
                SqlParam::from(if ev.repair_attempted { 1i64 } else { 0i64 }),
                SqlParam::from(ev.repair_status.as_deref()),
                SqlParam::from(ev.repaired_response.as_deref().map(redact_text)),
                SqlParam::from(ev.final_verdict.as_str()),
                SqlParam::from(ev.candidate_output_ref.as_deref()),
                SqlParam::from(ev.latency_s),
                SqlParam::from(ev.exit_status),
                SqlParam::from(if ev.timed_out { 1i64 } else { 0i64 }),
                SqlParam::from(now),
            ],
        )?;
        Ok(row_id)
    }

    pub fn verdict_events(&self, limit: i64) -> Result<Vec<Map<String, Value>>, String> {
        let conn = self.connect()?;
        conn.query(
            "SELECT * FROM verdict_events ORDER BY id DESC LIMIT ?",
            &[SqlParam::from(limit)],
        )
    }
}

#[derive(Debug, Clone)]
pub struct VerdictEventInput {
    pub run_id: String,
    pub gate_id: Option<String>,
    pub validator_name: String,
    pub validator_version: Option<String>,
    pub rendered_prompt: String,
    pub validation_input_ref: Option<String>,
    pub raw_response: String,
    pub initial_parse_status: String,
    pub parse_error_category: Option<String>,
    pub repair_attempted: bool,
    pub repair_status: Option<String>,
    pub repaired_response: Option<String>,
    pub final_verdict: String,
    pub candidate_output_ref: Option<String>,
    pub latency_s: Option<f64>,
    pub exit_status: Option<i64>,
    pub timed_out: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trip_and_upsert_and_forget() {
        let path = std::env::temp_dir().join(format!(
            "lope-memory-test-{}.db",
            lope_core::time::now()
        ));
        let mem = LopeMemory::open(&path).unwrap();
        let st0 = mem.stats().unwrap();
        assert_eq!(st0["total_findings"], 0);
        assert_eq!(st0["total_sessions"], 0);

        let mut ev = Map::new();
        ev.insert("claude".into(), Value::String("line 10".into()));
        let f1 = ConsensusFindingInput {
            hash: "abc1234567890def".into(),
            message: "SQL injection in login handler".into(),
            file: Some("src/auth.rs".into()),
            line: Some(42),
            end_line: Some(45),
            severity: "critical".into(),
            category: Some("security".into()),
            confidence_max: 0.9,
            consensus_score: 0.85,
            consensus_level: "likely".into(),
            agreement_count: 2,
            total_validators: 3,
            detected_by: vec!["claude".into(), "codex".into()],
            evidence: ev,
        };

        let (sid1, stored1) = mem
            .store_review_session(
                "review src/auth.rs",
                "security",
                "src/auth.rs",
                &["claude".into(), "codex".into()],
                std::slice::from_ref(&f1),
                120,
            )
            .unwrap();
        assert_eq!(sid1, 1);
        assert_eq!(stored1[0].seen_count, 1);

        // Re-detect with higher consensus level ("confirmed") -> seen_count bumps to 2, level promotes
        let mut f2 = f1.clone();
        f2.consensus_level = "confirmed".into();
        f2.consensus_score = 0.95;
        f2.detected_by = vec!["gemini".into()];
        let (_, stored2) = mem
            .store_review_session(
                "review src/auth.rs",
                "security",
                "src/auth.rs",
                &["gemini".into()],
                &[f2],
                150,
            )
            .unwrap();
        assert_eq!(stored2[0].seen_count, 2);
        assert_eq!(stored2[0].consensus_level, "confirmed");
        assert_eq!(
            stored2[0].detected_by,
            vec![
                "claude".to_string(),
                "codex".to_string(),
                "gemini".to_string()
            ]
        );

        let found = mem.search_findings("injection", 0.5, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].seen_count, 2);

        let by_file = mem.findings_for_file("src/auth.rs", 10).unwrap();
        assert_eq!(by_file.len(), 1);

        let hot = mem.hotspots(30, 10).unwrap();
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0]["file"], "src/auth.rs");
        assert_eq!(hot[0]["finding_count"], 1);
        assert_eq!(hot[0]["detection_count"], 2);

        let st1 = mem.stats().unwrap();
        assert_eq!(st1["total_findings"], 1);
        assert_eq!(st1["recurring_findings"], 1);
        assert_eq!(st1["confirmed_findings"], 1);
        assert_eq!(st1["flagged_files"], 1);
        assert_eq!(st1["total_sessions"], 2);

        let removed = mem.forget(Some("abc1234567890def"), None).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(mem.stats().unwrap()["total_findings"], 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a12_bidirectional_python_rust_and_four_concurrent_writers() {
        let path = std::env::temp_dir().join(format!(
            "lope-memory-a12-{}.db",
            lope_core::time::now()
        ));

        // 1. Rust writes to all 5 tables + 4 concurrent writers.
        let handles: Vec<_> = (0..4)
            .map(|w| {
                let p = path.clone();
                std::thread::spawn(move || {
                    let mem = LopeMemory::open(&p).expect("open db");
                    assert_eq!(mem.user_version().unwrap(), 1);
                    let mut ev = Map::new();
                    ev.insert(format!("v{w}"), Value::String(format!("line {w}")));
                    let f = ConsensusFindingInput {
                        hash: format!("hash{w:012}"),
                        message: format!("Concurrency finding {w}"),
                        file: Some(format!("src/mod_{w}.rs")),
                        line: Some(10 + w),
                        end_line: None,
                        severity: "high".into(),
                        category: Some("correctness".into()),
                        confidence_max: 0.8,
                        consensus_score: 0.8,
                        consensus_level: "confirmed".into(),
                        agreement_count: 2,
                        total_validators: 2,
                        detected_by: vec!["claude".into(), "codex".into()],
                        evidence: ev,
                    };
                    mem.store_review_session(
                        &format!("task {w}"),
                        "correctness",
                        &format!("src/mod_{w}.rs"),
                        &["claude".into(), "codex".into()],
                        &[f],
                        10,
                    )
                    .expect("store_review_session");
                    mem.store_gate_session(
                        &format!("gate {w}"),
                        "check",
                        ".lope/baseline.json",
                        true,
                        1,
                        0,
                        &json!({"worker": w}),
                    )
                    .expect("store_gate_session");
                    mem.store_verdict_event(&VerdictEventInput {
                        run_id: format!("run-{w}"),
                        gate_id: None,
                        validator_name: "claude".into(),
                        validator_version: None,
                        rendered_prompt: format!("prompt {w}"),
                        validation_input_ref: None,
                        raw_response: "response".into(),
                        initial_parse_status: "PASS".into(),
                        parse_error_category: None,
                        repair_attempted: false,
                        repair_status: None,
                        repaired_response: None,
                        final_verdict: "PASS".into(),
                        candidate_output_ref: None,
                        latency_s: Some(0.1),
                        exit_status: Some(0),
                        timed_out: false,
                    })
                    .expect("store_verdict_event");
                })
            })
            .collect();
        for h in handles {
            h.join().expect("concurrent writer thread");
        }

        // 2. Python reads all 5 tables written by Rust, verifies user_version == 1, and writes back to all 5 tables.
        let py_script = r#"
import os, sys, sqlite3
from pathlib import Path
sys.path.insert(0, os.path.expanduser("~/.lope"))
from lope.memory import LopeMemory
from lope.findings import ConsensusFinding, ConsensusLevel

db = Path(sys.argv[1])
mem = LopeMemory(db_path=db)
st = mem.stats()
assert st["total_findings"] == 4, st
assert st["total_sessions"] == 4, st
assert st["total_gate_sessions"] == 4, st
with mem._connect() as conn:
    uv = conn.execute("PRAGMA user_version").fetchone()[0]
    assert uv == 1, f"expected user_version=1, got {uv}"
    sf = conn.execute("SELECT COUNT(*) FROM session_findings").fetchone()[0]
    assert sf == 4, sf
    ve = conn.execute("SELECT COUNT(*) FROM verdict_events").fetchone()[0]
    assert ve == 4, ve

# Now Python writes to all 5 tables
f = ConsensusFinding(
    message="Python-written finding",
    file="src/py_written.py",
    line=99,
    severity="critical",
    category="security",
    confidence_max=0.95,
    confidence_avg=0.95,
    consensus_score=0.95,
    consensus_level=ConsensusLevel.CONFIRMED,
    agreement_count=2,
    total_validators=2,
    agreement_ratio=1.0,
    detected_by=["claude", "codex"],
    evidence={"claude": "line 99"},
)
mem.store_review_session(
    task="py review",
    focus="security",
    target_path="src/py_written.py",
    validators=["claude", "codex"],
    findings=[f],
)
mem.store_gate_session(
    task="py gate",
    mode="save",
    baseline_path=".lope/baseline.json",
    passed=True,
    gate_count=2,
    failed_count=0,
    payload={"from": "python"},
)
with mem._connect() as conn:
    conn.execute(
        "INSERT INTO verdict_events (schema_version, run_id, validator_name, prompt_template_version, rendered_prompt, task_spec_hash, raw_response, parser_version, initial_parse_status, final_verdict, created_at) VALUES ('1.0.0', 'py-run', 'codex', 'v1', 'p', 'h', 'r', 'v1', 'PASS', 'PASS', '2026-11-14T00:00:00Z')"
    )
    conn.commit()
"#;
        let out = std::process::Command::new("/usr/bin/python3")
            .arg("-I")
            .arg("-c")
            .arg(py_script)
            .arg(&path)
            .output()
            .expect("run python3");
        assert!(
            out.status.success(),
            "Python interoperability check failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );

        // 3. Rust reads back everything Python wrote and verifies user_version == 1.
        let mem = LopeMemory::open(&path).unwrap();
        assert_eq!(mem.user_version().unwrap(), 1);
        let st = mem.stats().unwrap();
        assert_eq!(st["total_findings"], 5);
        assert_eq!(st["total_sessions"], 5);
        assert_eq!(st["total_gate_sessions"], 5);
        let py_f = mem.findings_for_file("src/py_written.py", 10).unwrap();
        assert_eq!(py_f.len(), 1);
        assert_eq!(py_f[0].message, "Python-written finding");
        let ve = mem.verdict_events(10).unwrap();
        assert_eq!(ve.len(), 5);

        let _ = std::fs::remove_file(&path);
    }
}
