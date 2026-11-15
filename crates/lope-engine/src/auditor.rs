//! Post-mortem scorecard and journal writer (`auditor.py`, Sprint S7, `FR34`).

use crate::models::{ExecutionReport, VerdictStatus};
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct Auditor {
    journal_dir: PathBuf,
}

impl Auditor {
    pub fn new(journal_dir: Option<&Path>) -> Self {
        let dir = match journal_dir {
            Some(d) => d.to_path_buf(),
            None => {
                let lope_home = std::env::var("LOPE_HOME")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        let h = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                        PathBuf::from(h).join(".lope")
                    });
                lope_home.join("journal")
            }
        };
        Self { journal_dir: dir }
    }

    pub fn scorecard(&self, report: &ExecutionReport) -> String {
        let doc = &report.sprint_doc;
        let total_phases = doc.phases.len();
        let pass_count = report.count(VerdictStatus::Pass);
        let fix_count = report.count(VerdictStatus::NeedsFix);
        let fail_count = report.count(VerdictStatus::Fail);
        let infra_count = report.count(VerdictStatus::InfraError);

        let mut lines = vec![
            format!("Sprint: {}", doc.title),
            format!(
                "Phases: {total_phases}  (PASS={pass_count}, NEEDS_FIX={fix_count}, FAIL={fail_count}, INFRA={infra_count})"
            ),
            format!("Total duration: {:.1}s", report.total_duration_seconds),
            format!("Avg confidence: {:.2}", report.confidence_average()),
            "---".to_string(),
        ];

        let mut phases = doc.phases.clone();
        phases.sort_by_key(|p| p.index);
        for phase in phases {
            let Some(v) = &phase.verdict else {
                lines.push(format!("P{} {}: (not run)", phase.index, phase.name));
                continue;
            };
            lines.push(format!(
                "P{} {}: {} conf={:.2} dur={:.0}s",
                phase.index,
                phase.name,
                v.status.as_str(),
                v.confidence,
                v.duration_seconds
            ));
            if !phase.artifacts.is_empty() {
                lines.push(format!("  files: {}", phase.artifacts.join(", ")));
            }
            if !phase.checks.is_empty() {
                lines.push(format!("  tests: {}", phase.checks.join(", ")));
            }
            if !v.rationale.is_empty() {
                let first: String = v
                    .rationale
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .chars()
                    .take(160)
                    .collect();
                lines.push(format!("  rationale: {first}"));
            }
            if !v.required_fixes.is_empty() {
                lines.push(format!("  fixes: {} item(s)", v.required_fixes.len()));
            }
        }

        lines.push("---".to_string());
        lines.push(format!(
            "Overall: {}",
            if report.ok { "OK" } else { "ESCALATED" }
        ));
        if !report.error.is_empty() {
            lines.push(format!("Error: {}", report.error));
        }
        lines.join("\n")
    }

    pub fn write_journal(&self, report: &ExecutionReport) -> std::io::Result<PathBuf> {
        let filename = today_local_filename();
        let path = self.journal_dir.join(filename);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bullets = self.render_journal_bullets(report);
        let existed = path.exists();
        let ends_nl = if existed {
            file_ends_with_newline(&path)
        } else {
            true
        };
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        if existed && !ends_nl {
            f.write_all(b"\n")?;
        }
        f.write_all(bullets.join("\n").as_bytes())?;
        f.write_all(b"\n")?;
        eprintln!(
            "lope.auditor | [auditor] wrote {} bullet(s) to {}",
            bullets.len(),
            path.display()
        );
        Ok(path)
    }

    fn render_journal_bullets(&self, report: &ExecutionReport) -> Vec<String> {
        let doc = &report.sprint_doc;
        let title_tag = doc.title.replace(' ', "-");
        let pass_count = report.count(VerdictStatus::Pass);
        let total = doc.phases.len();
        let status_word = if report.ok { "COMPLETE" } else { "ESCALATED" };
        let mut bullets = vec![format!(
            "- **{} {status_word}** — {pass_count}/{total} phases PASS, avg confidence {:.2}, {:.0}s total. [[{title_tag}]] [[lope]] [[sprint-{}]]",
            doc.title,
            report.confidence_average(),
            report.total_duration_seconds,
            status_word.to_ascii_lowercase()
        )];
        let mut phases = doc.phases.clone();
        phases.sort_by_key(|p| p.index);
        for phase in phases {
            match &phase.verdict {
                None => bullets.push(format!("  - P{} {}: (not run)", phase.index, phase.name)),
                Some(v) => bullets.push(format!(
                    "  - P{} {}: {} conf={:.2} {:.0}s",
                    phase.index,
                    phase.name,
                    v.status.as_str(),
                    v.confidence,
                    v.duration_seconds
                )),
            }
        }
        if !report.error.is_empty() {
            let snippet: String = report.error.chars().take(200).collect();
            bullets.push(format!("  - Escalation: {snippet}"));
        }
        bullets
    }
}

fn today_local_filename() -> String {
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

fn file_ends_with_newline(path: &Path) -> bool {
    let Ok(mut f) = OpenOptions::new().read(true).open(path) else {
        return true;
    };
    let Ok(len) = f.seek(SeekFrom::End(0)) else {
        return true;
    };
    if len == 0 {
        return true;
    }
    if f.seek(SeekFrom::End(-1)).is_err() {
        return true;
    }
    let mut b = [0u8; 1];
    f.read_exact(&mut b).is_ok_and(|_| b[0] == b'\n')
}
