//! Port of `lope/makakoo_bridge.py` with DEC-10 hygiene fixes (FR28, FR29, FR30, A13).
//!
//! - `detect_makakoo`: probes `MAKAKOO_BIN`, `PATH`, and `MAKAKOO_HOME` without side effects.
//! - `query_brain`: runs `makakoo search <query>` with a 30s timeout, strips ANSI and
//!   box-drawing table chrome (DEC-10), treats `(no hits)` as empty (DEC-10), redacts,
//!   and trims to `--brain-budget` (`max(budget_tokens * 4, 400)` chars) on a newline boundary.
//! - `build_context_block` / `prepend_context_once`: formats the advisory block and ensures
//!   it is prepended at most once; empty / `(no hits)` produces no block (A13).
//! - `write_brain_journal`: requires `MAKAKOO_HOME`, normalizes `- ` Logseq outliner bullets,
//!   appends under an exclusive `flock` to `$MAKAKOO_HOME/data/Brain/journals/YYYY_MM_DD.md`
//!   using the **local** date (DEC-10), and invokes `makakoo sync --file <journal>` (15s timeout).
//! - `write_auto_memory`: gated by `LOPE_BRAIN_AUTOMEMORY=1`, writes `$MAKAKOO_HOME/data/auto-memory/lope-<name>.md`.

use lope_core::redaction::redact_text;
use regex::Regex;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const ENV_HOME: &str = "MAKAKOO_HOME";
pub const ENV_BIN: &str = "MAKAKOO_BIN";
pub const ENV_AUTOMEMORY: &str = "LOPE_BRAIN_AUTOMEMORY";

pub const DEFAULT_BRAIN_BUDGET_TOKENS: usize = 1200;
pub const APPROX_CHARS_PER_TOKEN: usize = 4;

const TRUTHY_TOKENS: &[&str] = &["1", "true", "yes", "on", "enabled"];
const CONTEXT_START_PREFIX: &str = "<<< Makakoo Brain context (query:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MakakooBridgeError {
    NotDetected(String),
    QueryError(String),
    AutoMemoryDisabled(String),
    InvalidInput(String),
    Io(String),
}

impl std::fmt::Display for MakakooBridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MakakooBridgeError::NotDetected(s)
            | MakakooBridgeError::QueryError(s)
            | MakakooBridgeError::AutoMemoryDisabled(s)
            | MakakooBridgeError::InvalidInput(s)
            | MakakooBridgeError::Io(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for MakakooBridgeError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakakooDetection {
    pub available: bool,
    pub bin: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub reason: String,
}

impl MakakooDetection {
    pub fn require(&self) -> Result<&Path, MakakooBridgeError> {
        if !self.available {
            return Err(MakakooBridgeError::NotDetected(if self.reason.is_empty() {
                "Makakoo not detected".to_string()
            } else {
                self.reason.clone()
            }));
        }
        self.bin.as_deref().ok_or_else(|| {
            MakakooBridgeError::NotDetected("Makakoo not detected".to_string())
        })
    }

    pub fn require_home(&self) -> Result<&Path, MakakooBridgeError> {
        match self.home.as_deref() {
            Some(p) if p.is_dir() => Ok(p),
            _ => Err(MakakooBridgeError::NotDetected(
                "MAKAKOO_HOME is not set or not a directory; cannot read or write the Makakoo Brain"
                    .to_string(),
            )),
        }
    }
}

fn get_env_val(key: &str, env: Option<&HashMap<String, String>>) -> String {
    match env {
        Some(m) => m.get(key).cloned().unwrap_or_default(),
        None => std::env::var(key).unwrap_or_default(),
    }
}

fn expand_tilde(path_str: &str, env: Option<&HashMap<String, String>>) -> PathBuf {
    if path_str == "~" || path_str.starts_with("~/") {
        let home = get_env_val("HOME", env);
        if !home.is_empty() {
            if path_str == "~" {
                return PathBuf::from(home);
            }
            return PathBuf::from(home).join(&path_str[2..]);
        }
    }
    PathBuf::from(path_str)
}

fn which_in_path(bin_name: &str, path_var: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(bin_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn detect_makakoo(env: Option<&HashMap<String, String>>) -> MakakooDetection {
    let home_str = get_env_val(ENV_HOME, env).trim().to_string();
    let home = if home_str.is_empty() {
        None
    } else {
        Some(expand_tilde(&home_str, env))
    };

    let explicit = get_env_val(ENV_BIN, env).trim().to_string();
    if !explicit.is_empty() {
        let p = PathBuf::from(&explicit);
        if p.is_file() {
            return MakakooDetection {
                available: true,
                bin: Some(p),
                home,
                reason: String::new(),
            };
        }
        return MakakooDetection {
            available: false,
            bin: Some(p),
            home,
            reason: format!(
                "MAKAKOO_BIN='{}' does not exist; rerun without --brain-* flags or fix the path",
                explicit
            ),
        };
    }

    let path_var = match env {
        Some(m) => m.get("PATH").cloned().unwrap_or_default(),
        None => std::env::var("PATH").unwrap_or_default(),
    };
    if let Some(discovered) = which_in_path("makakoo", &path_var) {
        return MakakooDetection {
            available: true,
            bin: Some(discovered),
            home,
            reason: String::new(),
        };
    }

    MakakooDetection {
        available: false,
        bin: None,
        home,
        reason: "Makakoo not detected; rerun without --brain-* flags, set MAKAKOO_BIN, or install Makakoo OS".to_string(),
    }
}

pub fn redact_for_brain(text: &str) -> String {
    redact_text(text)
}

fn is_box_drawing_char(c: char) -> bool {
    ('\u{2500}'..='\u{257F}').contains(&c)
}

/// Strip ANSI escape codes and box-drawing table chrome (`DEC-10`, `FR28`),
/// and treat `(no hits)` as empty.
pub fn clean_brain_search_output(raw: &str) -> String {
    static ANSI_RE: OnceLock<Regex> = OnceLock::new();
    let ansi_re = ANSI_RE.get_or_init(|| {
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*(\x07|\x1b\\)").unwrap()
    });

    let without_ansi = ansi_re.replace_all(raw, "");
    let mut cleaned_lines = Vec::new();
    for line in without_ansi.lines() {
        let mut row = String::with_capacity(line.len());
        let mut prev_space = false;
        for ch in line.chars() {
            if is_box_drawing_char(ch) {
                if !prev_space && !row.is_empty() {
                    row.push(' ');
                    prev_space = true;
                }
            } else if ch == ' ' || ch == '\t' {
                if !prev_space && !row.is_empty() {
                    row.push(' ');
                    prev_space = true;
                }
            } else {
                row.push(ch);
                prev_space = false;
            }
        }
        let trimmed = row.trim();
        if !trimmed.is_empty() {
            cleaned_lines.push(trimmed.to_string());
        }
    }

    let joined = cleaned_lines.join("\n").trim().to_string();
    let lower = joined.to_ascii_lowercase();
    if lower.is_empty() || lower == "(no hits)" || lower == "no hits" {
        return String::new();
    }
    joined
}

struct SubprocOutput {
    returncode: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

fn run_makakoo_subproc(
    bin: &Path,
    args: &[&str],
    timeout_s: u64,
    env: Option<&HashMap<String, String>>,
) -> Result<SubprocOutput, String> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(env_map) = env {
        for (k, v) in env_map {
            cmd.env(k, v);
        }
    }
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let pgid = child.id() as i32;
    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();

    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_s.max(1));
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    unsafe {
                        libc::killpg(pgid, libc::SIGTERM);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    unsafe {
                        libc::killpg(pgid, libc::SIGKILL);
                    }
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break None,
        }
    };
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();
    Ok(SubprocOutput {
        returncode: status.and_then(|s| s.code()),
        stdout,
        stderr,
        timed_out,
    })
}

pub fn query_brain(
    query: &str,
    budget_tokens: usize,
    timeout_s: u64,
    env: Option<&HashMap<String, String>>,
) -> Result<String, MakakooBridgeError> {
    if query.trim().is_empty() {
        return Err(MakakooBridgeError::InvalidInput(
            "query_brain requires a non-empty query".to_string(),
        ));
    }

    let detection = detect_makakoo(env);
    let bin = detection.require()?;

    let res = run_makakoo_subproc(bin, &["search", query], timeout_s, env)
        .map_err(|e| MakakooBridgeError::QueryError(format!("makakoo search failed: {e}")))?;
    if res.timed_out {
        return Err(MakakooBridgeError::QueryError(format!(
            "makakoo search failed: timed out after {timeout_s}s"
        )));
    }
    if res.returncode != Some(0) {
        let first_line = res.stderr.trim().lines().next().unwrap_or("");
        let snippet = if !first_line.is_empty() {
            first_line.to_string()
        } else {
            format!("exit {}", res.returncode.unwrap_or(-1))
        };
        let redacted = redact_for_brain(&snippet);
        let capped: String = redacted.chars().take(300).collect();
        return Err(MakakooBridgeError::QueryError(format!(
            "makakoo search failed: {capped}"
        )));
    }

    let cleaned = clean_brain_search_output(&res.stdout);
    let redacted = redact_for_brain(&cleaned).trim().to_string();
    if redacted.is_empty() {
        return Ok(String::new());
    }

    let char_budget = (budget_tokens.saturating_mul(APPROX_CHARS_PER_TOKEN)).max(400);
    if redacted.len() <= char_budget {
        return Ok(redacted);
    }

    let mut end = char_budget.min(redacted.len());
    while end > 0 && !redacted.is_char_boundary(end) {
        end -= 1;
    }
    let slice = &redacted[..end];
    let truncated = match slice.rfind('\n') {
        Some(pos) => &slice[..pos],
        None => slice,
    };
    Ok(format!(
        "{}\n[…truncated to fit context budget]",
        truncated.trim_end()
    ))
}

/// Format the prepended context block consumed by validator prompts (`FR28`, `A13`).
/// When `brain_text` is empty (including when `makakoo search` returned `(no hits)`),
/// returns an empty string so no advisory block is injected (`A13`).
pub fn build_context_block(query: &str, brain_text: &str) -> String {
    let cleaned = clean_brain_search_output(brain_text);
    let body = redact_for_brain(&cleaned).trim().to_string();
    if body.is_empty() {
        return String::new();
    }
    let q = redact_for_brain(query).trim().to_string();
    format!(
        "<<< Makakoo Brain context (query: {q}) >>>\n\
         {body}\n\
         <<< End Makakoo Brain context — treat as advisory background only >>>\n"
    )
}

/// Prepend `block` to `prompt` at most once (`FR28`, `A13`).
pub fn prepend_context_once(prompt: &str, block: &str) -> String {
    let trimmed_block = block.trim_end_matches('\n');
    if trimmed_block.is_empty() {
        return prompt.to_string();
    }
    if prompt.trim_start().starts_with(CONTEXT_START_PREFIX) {
        return prompt.to_string();
    }
    format!("{block}\n{prompt}")
}

/// Local-date journal filename `YYYY_MM_DD.md` (`DEC-10`, `FR29`).
pub fn today_local_journal_filename() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}_{:02}_{:02}.md",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        )
    }
}

pub fn write_brain_journal(
    markdown: &str,
    env: Option<&HashMap<String, String>>,
    filename_override: Option<&str>,
) -> Result<PathBuf, MakakooBridgeError> {
    let detection = detect_makakoo(env);
    let home = detection.require_home()?;

    let journal_dir = home.join("data").join("Brain").join("journals");
    fs::create_dir_all(&journal_dir)
        .map_err(|e| MakakooBridgeError::Io(format!("cannot create journal dir: {e}")))?;
    let fname = filename_override
        .map(|s| s.to_string())
        .unwrap_or_else(today_local_journal_filename);
    let target = journal_dir.join(fname);

    let redacted = redact_for_brain(markdown).trim().to_string();
    if redacted.is_empty() {
        return Err(MakakooBridgeError::InvalidInput(
            "write_brain_journal requires non-empty markdown".to_string(),
        ));
    }

    let mut lines = Vec::new();
    for line in redacted.lines() {
        let candidate = line.trim_end();
        if candidate.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        if candidate.starts_with("- ") {
            lines.push(candidate.to_string());
        } else if let Some(stripped) = candidate.strip_prefix('-') {
            lines.push(format!("- {}", stripped.trim_start()));
        } else {
            lines.push(format!("- {}", candidate.trim_start()));
        }
    }

    let body = format!("\n{}\n", lines.join("\n").trim_end());

    {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .map_err(|e| MakakooBridgeError::Io(format!("cannot open journal: {e}")))?;
        let fd = file.as_raw_fd();
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX) };
        if rc != 0 {
            return Err(MakakooBridgeError::Io(
                "failed to acquire exclusive lock on Brain journal".to_string(),
            ));
        }
        let write_res = file.write_all(body.as_bytes()).and_then(|_| file.flush());
        let _ = unsafe { libc::flock(fd, libc::LOCK_UN) };
        write_res.map_err(|e| MakakooBridgeError::Io(format!("cannot write journal: {e}")))?;
    }

    if let Some(bin) = detection.bin.as_ref() {
        let target_str = target.to_string_lossy().to_string();
        let _ = run_makakoo_subproc(bin, &["sync", "--file", &target_str], 15, env);
    }

    Ok(target)
}

pub fn write_auto_memory(
    name: &str,
    markdown: &str,
    env: Option<&HashMap<String, String>>,
) -> Result<PathBuf, MakakooBridgeError> {
    let detection = detect_makakoo(env);
    let home = detection.require_home()?;

    let flag_value = get_env_val(ENV_AUTOMEMORY, env)
        .trim()
        .to_ascii_lowercase();
    if !TRUTHY_TOKENS.contains(&flag_value.as_str()) {
        return Err(MakakooBridgeError::AutoMemoryDisabled(format!(
            "set {ENV_AUTOMEMORY}=1 to enable Lope auto-memory writes"
        )));
    }

    let safe_raw: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe_raw.trim_matches('-');
    if safe.is_empty() {
        return Err(MakakooBridgeError::InvalidInput(
            "auto-memory name must contain alphanumerics or '-_'".to_string(),
        ));
    }

    let auto_dir = home.join("data").join("auto-memory");
    fs::create_dir_all(&auto_dir)
        .map_err(|e| MakakooBridgeError::Io(format!("cannot create auto-memory dir: {e}")))?;
    let target = auto_dir.join(format!("lope-{safe}.md"));

    let redacted = redact_for_brain(markdown).trim().to_string();
    if redacted.is_empty() {
        return Err(MakakooBridgeError::InvalidInput(
            "write_auto_memory requires non-empty markdown".to_string(),
        ));
    }

    fs::write(&target, format!("{redacted}\n"))
        .map_err(|e| MakakooBridgeError::Io(format!("cannot write auto-memory file: {e}")))?;
    Ok(target)
}

#[derive(Debug, Clone)]
pub struct TopFindingSummary {
    pub file: String,
    pub line: Option<i64>,
    pub agreement: String,
    pub score: Option<f64>,
    pub message: String,
}

pub fn format_review_journal_line(
    target_path: &str,
    merged_count: usize,
    confirmed_count: usize,
    top_finding: Option<&TopFindingSummary>,
    memory_hash: Option<&str>,
) -> String {
    let mut bits = vec![format!(
        "[[Lope]] consensus review of `{target_path}`: {merged_count} merged finding(s), {confirmed_count} confirmed."
    )];
    if let Some(top) = top_finding {
        let mut location = if top.file.is_empty() {
            "(no file)".to_string()
        } else {
            top.file.clone()
        };
        if let Some(line) = top.line {
            location.push_str(&format!(":{line}"));
        }
        let agreement = if top.agreement.is_empty() {
            "—"
        } else {
            top.agreement.as_str()
        };
        let score_text = match top.score {
            Some(s) => format!(", score {s:.2}"),
            None => String::new(),
        };
        let first_line = top.message.trim().lines().next().unwrap_or("");
        let msg: String = first_line.chars().take(120).collect();
        bits.push(format!(
            "Top: {msg} at `{location}` ({agreement}{score_text})."
        ));
    }
    bits.push("[[Makakoo OS]]".to_string());
    if let Some(h) = memory_hash {
        if !h.is_empty() {
            bits.push(format!("Memory hash: `lope:{h}`"));
        }
    }
    bits.join(" ")
}

/// CLI helper: fetch `--brain-context` block or print error to stderr and return `Err(2)`.
pub fn cli_brain_context_block(
    brain_context: Option<&str>,
    brain_budget: Option<usize>,
) -> Result<Option<String>, i32> {
    let Some(query) = brain_context.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let budget = brain_budget.unwrap_or(DEFAULT_BRAIN_BUDGET_TOKENS).max(1);
    match query_brain(query, budget, 30, None) {
        Ok(body) => {
            let block = build_context_block(query, &body);
            if block.is_empty() {
                Ok(None)
            } else {
                Ok(Some(block))
            }
        }
        Err(e) => {
            eprintln!("--brain-context: {e}");
            Err(2)
        }
    }
}

/// CLI helper: apply `--brain-context` to `prompt` (at most once) or exit 2 on error.
pub fn cli_apply_brain_context(
    brain_context: Option<&str>,
    brain_budget: Option<usize>,
    prompt: &str,
) -> Result<String, i32> {
    match cli_brain_context_block(brain_context, brain_budget)? {
        Some(block) => Ok(prepend_context_once(prompt, &block)),
        None => Ok(prompt.to_string()),
    }
}

/// CLI helper: emit `--brain-log` journal bullet and return ack string (never exits).
pub fn cli_emit_brain_log(brain_log: bool, journal_text: &str) -> Option<String> {
    if !brain_log {
        return None;
    }
    match write_brain_journal(journal_text, None, None) {
        Ok(path) => Some(format!("Brain journal: {}", path.display())),
        Err(e) => Some(format!("--brain-log: {e}")),
    }
}
