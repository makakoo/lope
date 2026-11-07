//! `lope team [list|enable|disable|remove]` (`cli.py:_cmd_team` and helpers).
//! `add`, `test` and `health` stay delegated for now.

use super::{discover, Native, Out};
use clap::ArgMatches;
use lope_core::config::{self, LopeCfg};
use lope_core::output::ljust_str;
use serde_json::Value;

const BUILTINS: [&str; 5] = ["aider", "claude", "codex", "gemini", "opencode"];
const AUTO: [&str; 10] =
    ["vibe", "ollama", "goose", "interpreter", "llama-cpp", "gh-copilot", "amazon-q", "pi", "qwen", "agy"];

fn names(cfg: &LopeCfg) -> Vec<String> {
    cfg.validators.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

fn pname(p: &Value) -> Option<&str> {
    p.get("name").and_then(Value::as_str)
}

fn source(name: &str, cfg: &LopeCfg) -> String {
    if BUILTINS.contains(&name) {
        return "(built-in)".into();
    }
    if let Some(p) = cfg.providers.iter().find(|p| pname(p) == Some(name)) {
        return format!("(custom {})", p.get("type").map(config::py_str).unwrap_or_else(|| "?".into()));
    }
    if AUTO.contains(&name) {
        return "(auto)".into();
    }
    "(?)".into()
}

fn known(name: &str, cfg: &LopeCfg) -> bool {
    BUILTINS.contains(&name) || cfg.providers.iter().any(|p| pname(p) == Some(name)) || AUTO.contains(&name)
}

fn repr(s: &str) -> String {
    lope_state::registry::py_repr_str(s)
}

fn active_lines(cfg: &LopeCfg, out: &mut Out) {
    let n = names(cfg);
    out.line(format!("     Active validators ({}): {}", n.len(), n.join(", ")));
    if !cfg.primary.is_empty() {
        out.line(format!("     Primary: {}", cfg.primary));
    }
}

pub fn run(sub: Option<&str>, m: &ArgMatches) -> Native {
    let path = config::default_path();
    let loaded = config::load(&path);
    if let Some(c) = &loaded {
        if c.validators.iter().any(|v| !v.is_string()) || c.providers.iter().any(|p| !p.is_object()) {
            return Native::Delegate("unusual config shape".into());
        }
    }
    let mut cfg = loaded.unwrap_or_else(|| {
        let mut c = LopeCfg::defaults();
        c.timeout = Value::from(480);
        c
    });
    let args: Vec<String> = m
        .try_get_many::<std::ffi::OsString>("names")
        .ok()
        .flatten()
        .map(|v| v.map(|s| s.to_string_lossy().into_owned()).collect())
        .or_else(|| {
            m.try_get_one::<std::ffi::OsString>("name").ok().flatten().map(|s| vec![s.to_string_lossy().into_owned()])
        })
        .unwrap_or_default();
    if args.iter().any(|a| a.chars().any(|c| c.is_control() || c == '\\' || c == '\'' || c == '"')) {
        return Native::Delegate("name needs Python repr".into());
    }
    let mut out = Out::new();
    let save = |cfg: &LopeCfg| config::save(cfg, &path).map_err(|e| e.to_string());
    match sub.unwrap_or("list") {
        "enable" => {
            let mut changed = vec![];
            for raw in &args {
                let name = raw.trim();
                if name.is_empty() {
                    continue;
                }
                if !known(name, &cfg) {
                    drop(out);
                    eprintln!(
                        "ERROR: cannot enable unknown teammate {}.\n       Built-ins: {}.\n       Custom teammates must be added first with `lope team add {name} --cmd ...`.",
                        repr(name),
                        BUILTINS.join(", ")
                    );
                    return Native::Exit(2);
                }
                if !names(&cfg).iter().any(|n| n == name) {
                    cfg.validators.push(Value::from(name));
                    changed.push(name.to_string());
                }
            }
            if cfg.validators.is_empty() {
                drop(out);
                eprintln!("ERROR: no teammate names supplied");
                return Native::Exit(2);
            }
            let primary = m
                .try_get_one::<std::ffi::OsString>("--primary")
                .ok()
                .flatten()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty());
            let n = names(&cfg);
            if let Some(p) = primary {
                if !n.contains(&p) {
                    drop(out);
                    eprintln!("ERROR: --primary {} must be one of the enabled validators.", repr(&p));
                    return Native::Exit(2);
                }
                cfg.primary = p;
            } else if cfg.primary.is_empty() || !n.contains(&cfg.primary) {
                cfg.primary = n[0].clone();
            }
            if let Err(e) = save(&cfg) {
                return Native::Delegate(format!("save failed: {e}"));
            }
            if changed.is_empty() {
                out.line("[OK] already enabled.");
            } else {
                out.line(format!("[OK] enabled {}.", changed.join(", ")));
            }
            active_lines(&cfg, &mut out);
            Native::Exit(0)
        }
        "disable" => {
            let requested: Vec<String> = args.iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
            if requested.is_empty() {
                drop(out);
                eprintln!("ERROR: no teammate names supplied");
                return Native::Exit(2);
            }
            let active = names(&cfg);
            let missing: Vec<&String> = requested.iter().filter(|n| !active.contains(n)).collect();
            if !missing.is_empty() {
                drop(out);
                let a = if active.is_empty() { "(none)".to_string() } else { active.join(", ") };
                eprintln!(
                    "ERROR: not active: {}.\n       Active: {a}",
                    missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                );
                return Native::Exit(1);
            }
            cfg.validators.retain(|v| !requested.iter().any(|r| v.as_str() == Some(r)));
            if requested.contains(&cfg.primary) {
                cfg.primary = names(&cfg).first().cloned().unwrap_or_default();
            }
            if let Err(e) = save(&cfg) {
                return Native::Delegate(format!("save failed: {e}"));
            }
            out.line(format!("[OK] disabled {}.", requested.join(", ")));
            if cfg.validators.is_empty() {
                out.line(
                    "     No active validators. Enable one with `lope team enable claude` or add a custom teammate.",
                );
            } else {
                active_lines(&cfg, &mut out);
            }
            Native::Exit(0)
        }
        "remove" => {
            let name = args.first().map(|s| s.trim().to_string()).unwrap_or_default();
            if name.is_empty() {
                drop(out);
                eprintln!("ERROR: name required");
                return Native::Exit(2);
            }
            let before = cfg.providers.len();
            cfg.providers.retain(|p| pname(p) != Some(&name));
            let removed_provider = cfg.providers.len() < before;
            let was_validator = names(&cfg).contains(&name);
            if was_validator {
                cfg.validators.retain(|v| v.as_str() != Some(&name));
            }
            let was_primary = cfg.primary == name;
            if was_primary {
                cfg.primary = names(&cfg).first().cloned().unwrap_or_default();
            }
            if !removed_provider && !was_validator {
                drop(out);
                eprintln!(
                    "ERROR: no teammate named {} found in providers or validators.\n       Run `lope team list` to see who's on the team.",
                    repr(&name)
                );
                return Native::Exit(1);
            }
            if let Err(e) = save(&cfg) {
                return Native::Delegate(format!("save failed: {e}"));
            }
            let mut bits = vec![];
            if removed_provider {
                bits.push("custom provider config");
            }
            if was_validator {
                bits.push("active validators");
            }
            if was_primary {
                bits.push("primary role");
            }
            out.line(format!("[OK] removed {name} from {}.", bits.join(", ")));
            if cfg.validators.is_empty() {
                out.line("     No validators remaining — add one with `lope team add`.");
            } else {
                active_lines(&cfg, &mut out);
            }
            Native::Exit(0)
        }
        _ => {
            out.blank();
            out.line(lope_core::logo::boxed(""));
            out.line("\nLope — Your Validator Team");
            out.line("-".repeat(40));
            let active = names(&cfg);
            if active.is_empty() {
                out.line("\n  (no active validators yet — add one with `lope team add <name> ...`)");
            } else {
                out.line(format!("\nActive ({}):", active.len()));
                for n in &active {
                    let marker = if *n == cfg.primary { " ★ primary" } else { "" };
                    out.line(format!("  {} {}{marker}", ljust_str(n, 22), source(n, &cfg)));
                }
            }
            let disabled: Vec<&Value> =
                cfg.providers.iter().filter(|p| !active.iter().any(|a| Some(a.as_str()) == pname(p))).collect();
            if !disabled.is_empty() {
                out.line(format!("\nDisabled providers ({}):", disabled.len()));
                for e in disabled {
                    let nm = e.get("name").map(config::py_str).unwrap_or_else(|| "?".into());
                    let ty = e.get("type").map(config::py_str).unwrap_or_else(|| "?".into());
                    out.line(format!("  {} ({ty})", ljust_str(&nm, 22)));
                }
            }
            let avail: Vec<&str> = BUILTINS
                .iter()
                .copied()
                .filter(|b| !active.iter().any(|a| a == b) && lope_core::paths::which(b).is_some())
                .collect();
            let _ = discover::KNOWN_CLIS.len();
            if !avail.is_empty() {
                out.line(format!("\nAvailable built-ins ({}):", avail.len()));
                for b in avail {
                    out.line(format!("  {} enable: lope team enable {b}", ljust_str(b, 22)));
                }
            }
            out.blank();
            out.line("Add a teammate:");
            out.line("  lope team add <name> --cmd \"binary --flag {prompt}\"");
            out.line("  lope team add <name> --url URL --model MODEL --key-env OPENAI_API_KEY");
            out.line("Enable a built-in:");
            out.line("  lope team enable codex opencode");
            out.blank();
            Native::Exit(0)
        }
    }
}
