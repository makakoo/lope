//! `lope/divide.py`: file walker, diff hunk splitter, and role lenses for `lope review`.

use lope_core::redaction::redact_text;
use lope_providers::request_plan::{pack_units, semantic_units, SemanticUnit};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const DEFAULT_MAX_CHARS: usize = 16_000;
pub const DEFAULT_MAX_FILE_BYTES: u64 = 256_000;

const BINARY_EXTENSIONS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".tiff", ".ico",
    ".pdf", ".zip", ".gz", ".tgz", ".bz2", ".xz", ".7z", ".rar",
    ".so", ".dylib", ".dll", ".a", ".o", ".obj", ".exe",
    ".woff", ".woff2", ".ttf", ".otf", ".eot",
    ".mp3", ".mp4", ".mov", ".avi", ".webm", ".mkv", ".wav", ".flac",
    ".class", ".jar", ".whl", ".pyc",
    ".db", ".sqlite", ".sqlite3", ".lock",
    ".bin", ".dat",
];

const SKIP_DIRECTORIES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "__pycache__",
    ".venv",
    "venv",
    "node_modules",
    "dist",
    "build",
    "target",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".idea",
    ".vscode",
    ".DS_Store",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunk {
    pub path: String,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
    pub chunk_index: usize,
    pub chunk_total: usize,
}

impl FileChunk {
    pub fn label(&self) -> String {
        if self.chunk_total <= 1 {
            self.path.clone()
        } else {
            format!(
                "{} (chunk {}/{}: lines {}-{})",
                self.path,
                self.chunk_index + 1,
                self.chunk_total,
                self.start_line,
                self.end_line
            )
        }
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "path": self.path,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "chunk_index": self.chunk_index,
            "chunk_total": self.chunk_total,
            "content_chars": self.content.chars().count(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkChunk {
    pub path: String,
    pub hunk_index: usize,
    pub content: String,
    pub new_start: usize,
    pub new_lines: usize,
    pub old_start: usize,
    pub old_lines: usize,
}

impl HunkChunk {
    pub fn new_end(&self) -> usize {
        self.new_start.max(self.new_start + self.new_lines.saturating_sub(1))
    }

    pub fn label(&self) -> String {
        format!(
            "{} (hunk {}: new lines {}-{})",
            self.path,
            self.hunk_index + 1,
            self.new_start,
            self.new_end()
        )
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "path": self.path,
            "hunk_index": self.hunk_index,
            "new_start": self.new_start,
            "new_lines": self.new_lines,
            "new_end": self.new_end(),
            "old_start": self.old_start,
            "old_lines": self.old_lines,
            "content_chars": self.content.chars().count(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedReviewChunk {
    pub content: String,
    pub labels: Vec<String>,
    pub paths: Vec<String>,
    pub index: usize,
    pub total: usize,
    pub anchor_line: Option<usize>,
}

impl PackedReviewChunk {
    pub fn label(&self) -> String {
        let mut preview = self.labels.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
        if self.labels.len() > 3 {
            preview.push_str(&format!(", +{} more", self.labels.len() - 3));
        }
        format!("packed chunk {}/{}: {}", self.index + 1, self.total, preview)
    }

    pub fn path(&self) -> String {
        if self.paths.len() == 1 {
            self.paths[0].clone()
        } else {
            "(packed sources)".to_string()
        }
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "index": self.index,
            "total": self.total,
            "labels": self.labels,
            "paths": self.paths,
            "content_bytes": self.content.len(),
            "anchor_line": self.anchor_line,
        })
    }
}

pub fn split_files(
    target: &Path,
    max_chars: usize,
    max_file_bytes: u64,
) -> std::io::Result<(Vec<FileChunk>, Vec<SkippedFile>)> {
    if !target.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            target.display().to_string(),
        ));
    }

    let binary_exts: HashSet<&str> = BINARY_EXTENSIONS.iter().copied().collect();
    let skip_dirs: HashSet<&str> = SKIP_DIRECTORIES.iter().copied().collect();

    let files = if target.is_file() {
        vec![target.to_path_buf()]
    } else {
        let mut list = Vec::new();
        let boundary = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
        iter_files(target, &skip_dirs, &boundary, &mut list);
        list.sort();
        list
    };

    let mut chunks = Vec::new();
    let mut skipped = Vec::new();

    for path in files {
        let rel = safe_relpath(&path, target);
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(exc) => {
                skipped.push(SkippedFile {
                    path: rel,
                    reason: "unreadable".into(),
                    detail: exc.to_string(),
                });
                continue;
            }
        };

        if meta.len() > max_file_bytes {
            skipped.push(SkippedFile {
                path: rel,
                reason: "too_large".into(),
                detail: format!("{} bytes > {max_file_bytes}", meta.len()),
            });
            continue;
        }

        let suffix = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_ascii_lowercase()))
            .unwrap_or_default();
        if binary_exts.contains(suffix.as_str()) {
            let orig_suffix = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{e}"))
                .unwrap_or_default();
            skipped.push(SkippedFile {
                path: rel,
                reason: "binary".into(),
                detail: orig_suffix,
            });
            continue;
        }

        let raw = match std::fs::read(&path) {
            Ok(b) => b,
            Err(exc) => {
                skipped.push(SkippedFile {
                    path: rel,
                    reason: "unreadable".into(),
                    detail: exc.to_string(),
                });
                continue;
            }
        };

        if raw.contains(&0) {
            skipped.push(SkippedFile {
                path: rel,
                reason: "binary".into(),
                detail: "NUL byte".into(),
            });
            continue;
        }

        let text = match String::from_utf8(raw) {
            Ok(s) => s,
            Err(err) => {
                let bytes = err.into_bytes();
                bytes.into_iter().map(|b| b as char).collect()
            }
        };

        chunks.extend(chunk_text(&rel, &text, max_chars));
    }

    Ok((chunks, skipped))
}

fn iter_files(root: &Path, skip_dirs: &HashSet<&str>, boundary: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();

    for entry in entries {
        if let Some(name) = entry.file_name().and_then(|n| n.to_str()) {
            if skip_dirs.contains(name) {
                continue;
            }
        }
        let Ok(ft) = std::fs::symlink_metadata(&entry) else {
            continue;
        };
        if ft.file_type().is_symlink() {
            let Ok(resolved) = std::fs::canonicalize(&entry) else {
                continue;
            };
            if !resolved.starts_with(boundary) {
                continue;
            }
            if resolved.is_file() {
                out.push(resolved);
            } else if resolved.is_dir() {
                iter_files(&resolved, skip_dirs, boundary, out);
            }
            continue;
        }
        if ft.is_dir() {
            iter_files(&entry, skip_dirs, boundary, out);
        } else if ft.is_file() {
            out.push(entry);
        }
    }
}

fn safe_relpath(path: &Path, root: &Path) -> String {
    if root.is_dir() {
        if let (Ok(rp), Ok(rr)) = (std::fs::canonicalize(path), std::fs::canonicalize(root)) {
            if let Ok(rel) = rp.strip_prefix(&rr) {
                return rel.display().to_string();
            }
        }
    }
    path.display().to_string()
}

fn chunk_text(path: &str, text: &str, max_chars: usize) -> Vec<FileChunk> {
    let max_chars = if max_chars == 0 { DEFAULT_MAX_CHARS } else { max_chars };
    let kind = if path.to_ascii_lowercase().ends_with(".py") { "python" } else { "auto" };
    let planned = pack_units(&semantic_units(text, path, kind), max_chars, 0);
    let total = planned.len();
    planned
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| FileChunk {
            path: path.to_string(),
            content: chunk.content,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            chunk_index: index,
            chunk_total: total,
        })
        .collect()
}

fn pack_review_units(
    units: &[SemanticUnit],
    paths_by_label: &HashMap<String, String>,
    anchors_by_label: Option<&HashMap<String, usize>>,
    max_bytes: usize,
) -> Vec<PackedReviewChunk> {
    let mut sorted_candidates: Vec<&String> = paths_by_label.keys().collect();
    sorted_candidates.sort_by_key(|k| std::cmp::Reverse(k.len()));

    let source_label = |label: &str| -> String {
        if paths_by_label.contains_key(label) {
            return label.to_string();
        }
        for candidate in &sorted_candidates {
            let prefix = format!("{candidate} part ");
            if label.starts_with(&prefix) {
                return (*candidate).clone();
            }
        }
        label.to_string()
    };

    let planned = pack_units(units, max_bytes, 0);
    let total = planned.len();
    let mut out = Vec::new();
    for (index, chunk) in planned.into_iter().enumerate() {
        let base_labels: Vec<String> = chunk.labels.iter().map(|l| source_label(l)).collect();
        let mut paths = Vec::new();
        for label in &base_labels {
            let p = paths_by_label.get(label).cloned().unwrap_or_else(|| label.clone());
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
        let anchor = if let Some(anchors) = anchors_by_label {
            if chunk.labels.len() == 1 {
                anchors.get(&base_labels[0]).copied()
            } else {
                None
            }
        } else {
            None
        };
        out.push(PackedReviewChunk {
            content: chunk.content,
            labels: chunk.labels,
            paths,
            index,
            total,
            anchor_line: anchor,
        });
    }
    out
}

pub fn pack_file_chunks(chunks: &[FileChunk], max_bytes: usize) -> Vec<PackedReviewChunk> {
    let mut units = Vec::new();
    let mut paths = HashMap::new();
    for chunk in chunks {
        let label = chunk.label();
        let header = format!("\n===== FILE: {label} =====\n");
        units.push(SemanticUnit::new(
            label.clone(),
            format!("{header}{}", chunk.content),
            chunk.start_line,
            chunk.end_line,
            "file",
        ));
        paths.insert(label, chunk.path.clone());
    }
    pack_review_units(&units, &paths, None, max_bytes)
}

pub fn pack_hunk_chunks(hunks: &[HunkChunk], max_bytes: usize) -> Vec<PackedReviewChunk> {
    let mut units = Vec::new();
    let mut paths = HashMap::new();
    let mut anchors = HashMap::new();
    for hunk in hunks {
        let label = hunk.label();
        let header = format!("\n===== DIFF HUNK: {label} =====\n");
        units.push(SemanticUnit::new(
            label.clone(),
            format!("{header}{}", hunk.content),
            hunk.new_start,
            hunk.new_end(),
            "diff",
        ));
        paths.insert(label.clone(), hunk.path.clone());
        anchors.insert(label, hunk.new_start);
    }
    pack_review_units(&units, &paths, Some(&anchors), max_bytes)
}

fn diff_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^diff --git a/(?P<a>\S+)\s+b/(?P<b>\S+)\s*$").unwrap())
}

fn new_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\+\+\+\s+b/(?P<path>.+?)\s*$").unwrap())
}

fn old_file_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^---\s+a/(?P<path>.+?)\s*$").unwrap())
}

fn hunk_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^@@\s+-(?P<old_start>\d+)(?:,(?P<old_lines>\d+))?\s+\+(?P<new_start>\d+)(?:,(?P<new_lines>\d+))?\s+@@",
        )
        .unwrap()
    })
}

pub fn split_diff_hunks(text: &str) -> Vec<HunkChunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current_path: Option<String> = None;
    let mut hunk_index_per_file: HashMap<String, usize> = HashMap::new();
    let mut in_hunk = false;
    let mut hunk_lines: Vec<&str> = Vec::new();
    let mut old_start = 0usize;
    let mut old_lines = 0usize;
    let mut new_start = 0usize;
    let mut new_lines = 1usize;

    let flush = |out: &mut Vec<HunkChunk>,
                 in_hunk: &mut bool,
                 current_path: &Option<String>,
                 hunk_index_per_file: &mut HashMap<String, usize>,
                 hunk_lines: &mut Vec<&str>,
                 old_start: usize,
                 old_lines: usize,
                 new_start: usize,
                 new_lines: usize| {
        if !*in_hunk || current_path.is_none() {
            hunk_lines.clear();
            *in_hunk = false;
            return;
        }
        let cp = current_path.as_ref().unwrap();
        let idx = *hunk_index_per_file.get(cp).unwrap_or(&0);
        let joined = hunk_lines.join("\n");
        let content = format!("{}\n", joined.trim_end_matches('\n'));
        out.push(HunkChunk {
            path: cp.clone(),
            hunk_index: idx,
            content,
            new_start,
            new_lines,
            old_start,
            old_lines,
        });
        hunk_index_per_file.insert(cp.clone(), idx + 1);
        hunk_lines.clear();
        *in_hunk = false;
    };

    for line in text.lines() {
        if let Some(caps) = diff_header_re().captures(line) {
            flush(
                &mut out,
                &mut in_hunk,
                &current_path,
                &mut hunk_index_per_file,
                &mut hunk_lines,
                old_start,
                old_lines,
                new_start,
                new_lines,
            );
            current_path = caps.name("b").map(|m| m.as_str().to_string());
            continue;
        }
        if let Some(caps) = new_file_re().captures(line) {
            flush(
                &mut out,
                &mut in_hunk,
                &current_path,
                &mut hunk_index_per_file,
                &mut hunk_lines,
                old_start,
                old_lines,
                new_start,
                new_lines,
            );
            current_path = caps.name("path").map(|m| m.as_str().to_string());
            continue;
        }
        if old_file_re().is_match(line) {
            continue;
        }
        if let Some(caps) = hunk_header_re().captures(line) {
            flush(
                &mut out,
                &mut in_hunk,
                &current_path,
                &mut hunk_index_per_file,
                &mut hunk_lines,
                old_start,
                old_lines,
                new_start,
                new_lines,
            );
            old_start = caps.name("old_start").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            old_lines = caps.name("old_lines").and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
            new_start = caps.name("new_start").and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            new_lines = caps.name("new_lines").and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
            hunk_lines = vec![line];
            in_hunk = true;
            continue;
        }
        if in_hunk {
            hunk_lines.push(line);
        }
    }

    flush(
        &mut out,
        &mut in_hunk,
        &current_path,
        &mut hunk_index_per_file,
        &mut hunk_lines,
        old_start,
        old_lines,
        new_start,
        new_lines,
    );
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleLens {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub prompt_prefix: &'static str,
}

impl RoleLens {
    pub fn to_dict(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
        })
    }
}

pub const ROLE_LENSES: &[RoleLens] = &[
    RoleLens {
        name: "security",
        title: "Security review",
        description: "Auth, input validation, secrets, OWASP-class issues.",
        prompt_prefix: "You are reviewing strictly through a SECURITY lens. Focus on auth bypass, input validation, secret handling, injection, authorization gaps, and unsafe deserialization. Skip style, perf, and pure-readability concerns — other reviewers cover those. Tag findings with `[security]` so the consensus tooling can group them.",
    },
    RoleLens {
        name: "correctness",
        title: "Correctness review",
        description: "Logic bugs, edge cases, off-by-one, data integrity.",
        prompt_prefix: "You are reviewing strictly through a CORRECTNESS lens. Hunt for logic bugs, edge cases, off-by-one errors, broken invariants, and silent data corruption. Skip security/perf — other reviewers own those. Tag findings with `[correctness]`.",
    },
    RoleLens {
        name: "performance",
        title: "Performance review",
        description: "Hot loops, allocations, IO, indexes, caching.",
        prompt_prefix: "You are reviewing strictly through a PERFORMANCE lens. Look for hot loops, unnecessary allocations, blocking IO on critical paths, missing indexes, cache misses, and quadratic algorithms. Tag findings with `[perf]`.",
    },
    RoleLens {
        name: "tests",
        title: "Test coverage review",
        description: "Missing tests, brittle tests, mock-real divergence.",
        prompt_prefix: "You are reviewing strictly through a TESTS lens. Identify missing test coverage, brittle assertions, mock-vs-prod divergence, and untested error paths. Tag findings with `[tests]`.",
    },
    RoleLens {
        name: "api-design",
        title: "API design review",
        description: "Surface ergonomics, breaking change risk, contracts.",
        prompt_prefix: "You are reviewing strictly through an API-DESIGN lens. Evaluate naming, parameter ordering, breaking-change risk, back-compat, and consumer ergonomics. Tag findings with `[api-design]`.",
    },
    RoleLens {
        name: "docs",
        title: "Docs review",
        description: "Missing docstrings, stale comments, README drift.",
        prompt_prefix: "You are reviewing strictly through a DOCS lens. Flag missing docstrings, stale comments, drift between README and behavior, and any contract that is not documented. Tag findings with `[docs]`.",
    },
    RoleLens {
        name: "ops",
        title: "Ops / operability review",
        description: "Logging, metrics, error handling, deployability.",
        prompt_prefix: "You are reviewing strictly through an OPS lens. Look at structured logging, metrics, alerting hooks, error surfacing, rollback behavior, and on-call ergonomics. Tag findings with `[ops]`.",
    },
    RoleLens {
        name: "ux",
        title: "UX review",
        description: "User-visible affordances, error messages, defaults.",
        prompt_prefix: "You are reviewing strictly through a UX lens. Evaluate user-visible affordances, error messages, sensible defaults, and surprising behavior. Tag findings with `[ux]`.",
    },
];

pub fn list_roles() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ROLE_LENSES.iter().map(|r| r.name).collect();
    names.sort_unstable();
    names
}

pub fn get_role(name: &str) -> Result<RoleLens, String> {
    let key = name.trim().to_ascii_lowercase();
    let canonical = match key.as_str() {
        "perf" | "speed" => "performance",
        "test" | "testing" => "tests",
        "doc" | "documentation" => "docs",
        "op" | "operations" | "infra" | "ci" => "ops",
        "sec" => "security",
        "design" => "ux",
        "api" => "api-design",
        other => other,
    };
    ROLE_LENSES
        .iter()
        .find(|r| r.name == canonical)
        .cloned()
        .ok_or_else(|| {
            format!(
                "Unknown role lens: {name:?}. Available: {}",
                list_roles().join(", ")
            )
        })
}

pub fn parse_roles(spec: &str) -> Result<Vec<RoleLens>, String> {
    if spec.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for raw in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let lens = get_role(raw)?;
        if !seen.contains(&lens.name) {
            seen.push(lens.name);
            out.push(lens);
        }
    }
    Ok(out)
}

pub fn assign_roles(validators: &[String], roles: &[RoleLens]) -> Result<BTreeMap<String, RoleLens>, String> {
    if validators.is_empty() {
        return Err("assign_roles requires at least one validator".into());
    }
    if roles.is_empty() {
        return Err("assign_roles requires at least one role".into());
    }
    let mut out = BTreeMap::new();
    for (idx, validator) in validators.iter().enumerate() {
        out.insert(validator.clone(), roles[idx % roles.len()].clone());
    }
    Ok(out)
}

pub fn build_role_prompt(role: &RoleLens, base_prompt: &str) -> String {
    let safe_prefix = redact_text(role.prompt_prefix).trim().to_string();
    let safe_base = redact_text(base_prompt).trim_end().to_string();
    format!("{safe_prefix}\n\n{safe_base}\n")
}
