//! `cli_discovery.py` + `selector.py` + `_cmd_configure`: the 15 known CLIs,
//! probed with `shutil.which`, and the interactive/non-interactive `configure`
//! wizard.

use super::{Native, Out};
use lope_core::config::{self, LopeCfg};
use lope_core::output::ljust_str;
use serde_json::Value;
use std::io::{BufRead, Write};

pub struct CliInfo {
    pub name: &'static str,
    pub binary: &'static str,
    pub display_name: &'static str,
    pub is_default: bool,
}

pub const KNOWN_CLIS: [CliInfo; 15] = [
    CliInfo { name: "claude", binary: "claude", display_name: "Claude Code", is_default: true },
    CliInfo { name: "opencode", binary: "opencode", display_name: "OpenCode", is_default: true },
    CliInfo { name: "gemini", binary: "gemini", display_name: "Gemini CLI", is_default: false },
    CliInfo { name: "codex", binary: "codex", display_name: "OpenAI Codex", is_default: false },
    CliInfo { name: "vibe", binary: "vibe", display_name: "Mistral Vibe", is_default: false },
    CliInfo { name: "aider", binary: "aider", display_name: "Aider", is_default: false },
    CliInfo { name: "ollama", binary: "ollama", display_name: "Ollama (local)", is_default: false },
    CliInfo { name: "goose", binary: "goose", display_name: "Goose (Block)", is_default: false },
    CliInfo { name: "interpreter", binary: "interpreter", display_name: "Open Interpreter", is_default: false },
    CliInfo { name: "llama-cpp", binary: "llama-cli", display_name: "llama.cpp", is_default: false },
    CliInfo { name: "gh-copilot", binary: "gh", display_name: "GitHub Copilot CLI", is_default: false },
    CliInfo { name: "amazon-q", binary: "q", display_name: "Amazon Q", is_default: false },
    CliInfo { name: "pi", binary: "pi", display_name: "pi (Traylinx)", is_default: false },
    CliInfo { name: "qwen", binary: "qwen", display_name: "Qwen Code", is_default: false },
    CliInfo { name: "agy", binary: "agy", display_name: "Agy", is_default: false },
];

pub fn discover() -> Vec<&'static CliInfo> {
    KNOWN_CLIS.iter().filter(|c| lope_core::paths::which(c.binary).is_some()).collect()
}

pub fn defaults<'a>(available: &[&'a CliInfo]) -> Vec<&'a CliInfo> {
    let defs: Vec<&'a CliInfo> = available.iter().copied().filter(|c| c.is_default).collect();
    if !defs.is_empty() {
        return defs;
    }
    available.iter().copied().take(2).collect()
}

fn is_interactive() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

pub fn run_selector(available: &[&'static CliInfo]) -> LopeCfg {
    let defs = defaults(available);
    if !is_interactive() {
        let mut cfg = LopeCfg::defaults();
        cfg.validators = defs.iter().map(|c| Value::String(c.name.to_string())).collect();
        cfg.primary = defs.first().map(|c| c.name.to_string()).unwrap_or_default();
        cfg.timeout = Value::from(480);
        cfg.parallel = true;
        return cfg;
    }

    let mut out = Out::new();
    out.blank();
    out.line("Lope — CLI validator setup");
    out.line("─".repeat(50));
    out.blank();
    out.line("Detected AI CLIs on this machine:");
    out.blank();

    let max_disp = available.iter().map(|c| c.display_name.len()).max().unwrap_or(0);
    for (i, cli) in available.iter().enumerate() {
        let idx = i + 1;
        let padded = ljust_str(cli.display_name, max_disp);
        if cli.is_default {
            out.line(format!("  [{idx}] {padded}  ({})  ● DEFAULT", cli.name));
        } else {
            out.line(format!("  [{idx}] {padded}  ({})", cli.name));
        }
    }

    let default_names: Vec<&str> = defs.iter().map(|c| c.name).collect();
    let default_display = if default_names.is_empty() {
        "[none detected]".to_string()
    } else {
        default_names.join(",")
    };

    out.blank();
    out.line(format!(
        "Select validators (comma-separated numbers, or Enter for defaults [{default_display}]):"
    ));
    out.flush();
    print!("> ");
    let _ = std::io::stdout().flush();

    let stdin = std::io::stdin();
    let mut raw_input = String::new();
    let _ = stdin.lock().read_line(&mut raw_input);
    let raw_input = raw_input.trim();

    let selected_names: Vec<String> = if raw_input.is_empty() {
        default_names.iter().map(|s| (*s).to_string()).collect()
    } else {
        let mut sel = Vec::new();
        for part in raw_input.split(',') {
            let p = part.trim();
            if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(n) = p.parse::<usize>() {
                    if n >= 1 && n <= available.len() {
                        sel.push(available[n - 1].name.to_string());
                    }
                }
            }
        }
        if sel.is_empty() {
            default_names.iter().map(|s| (*s).to_string()).collect()
        } else {
            sel
        }
    };

    out.blank();
    out.line("Primary validator (tie-breaker, Enter for [1]):");
    out.flush();
    print!("> ");
    let _ = std::io::stdout().flush();

    let mut raw_primary = String::new();
    let _ = stdin.lock().read_line(&mut raw_primary);
    let raw_primary = raw_primary.trim();

    let primary = if raw_primary.is_empty() {
        selected_names.first().cloned().unwrap_or_default()
    } else if raw_primary.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = raw_primary.parse::<usize>() {
            if n >= 1 && n <= selected_names.len() {
                selected_names[n - 1].clone()
            } else {
                selected_names.first().cloned().unwrap_or_default()
            }
        } else {
            selected_names.first().cloned().unwrap_or_default()
        }
    } else {
        raw_primary.to_string()
    };

    out.blank();
    out.line("Parallel mode? Runs all validators concurrently (Y/n, Enter for Y):");
    out.flush();
    print!("> ");
    let _ = std::io::stdout().flush();

    let mut raw_parallel = String::new();
    let _ = stdin.lock().read_line(&mut raw_parallel);
    let parallel = !matches!(raw_parallel.trim().to_ascii_lowercase().as_str(), "n" | "no");

    let mut cfg = LopeCfg::defaults();
    cfg.validators = selected_names.into_iter().map(Value::String).collect();
    cfg.primary = primary;
    cfg.timeout = Value::from(480);
    cfg.parallel = parallel;
    cfg
}

pub fn run_configure() -> Native {
    let mut out = Out::new();
    out.blank();
    out.line(lope_core::logo::mascot("let's set up your validators"));
    out.blank();
    let available = discover();
    if available.is_empty() {
        out.line("No AI CLIs detected. Install at least one of: claude, opencode, gemini, codex, aider");
        return Native::Exit(1);
    }
    out.flush();
    let cfg = run_selector(&available);
    let path = config::default_path();
    if let Err(e) = config::save(&cfg, &path) {
        return Native::Delegate(format!("configure save failed: {e}"));
    }
    let val_names: Vec<String> = cfg.validators.iter().map(config::py_str).collect();
    out.line(format!("\nConfig saved to {}", path.display()));
    out.line(format!("  Validators: {}", val_names.join(", ")));
    out.line(format!("  Primary: {}", cfg.primary));
    out.line(format!("  Parallel: {}", if cfg.parallel { "True" } else { "False" }));
    Native::Exit(0)
}
