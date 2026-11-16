//! S8 acceptance & parity tests: FR37 (`install`), FR38 (`update`/`upgrade` delegated), FR39 (`docs`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("canonicalize repo root")
}

fn prep_optional_host_dirs(home: &Path) {
    for d in [".vibe", ".qwen", ".pi"] {
        fs::create_dir_all(home.join(d)).unwrap();
    }
}

fn collect_host_layout(home: &Path) -> BTreeMap<PathBuf, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) {
        let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            // Exclude install backups (.local), runtime state (.lope), and macOS Python cache (Library).
            if rel.starts_with(".local") || rel.starts_with(".lope") || rel.starts_with("Library") {
                continue;
            }
            let meta = fs::symlink_metadata(&path).unwrap();
            let ft = meta.file_type();
            if ft.is_symlink() {
                let target = fs::read_link(&path).unwrap();
                out.insert(rel, format!("symlink:{}", target.display()));
            } else if ft.is_dir() {
                out.insert(rel, "dir".to_string());
                walk(base, &path, out);
            } else {
                out.insert(rel, "file".to_string());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(home, home, &mut out);
    out
}

fn scan_no_python_m_lope(root: &Path) {
    fn walk(dir: &Path) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if let Ok(meta) = fs::metadata(&path) {
                if meta.is_dir() {
                    walk(&path);
                } else if meta.is_file() {
                    if let Ok(txt) = fs::read_to_string(&path) {
                        assert!(
                            !txt.contains("python3 -m lope"),
                            "stale `python3 -m lope` found in {}",
                            path.display()
                        );
                        assert!(!txt.contains("PYTHONPATH="), "stale `PYTHONPATH=` found in {}", path.display());
                    }
                }
            }
        }
    }
    for sub in [".claude", ".codex", ".gemini", ".config/opencode", ".cursor", ".vibe", ".qwen", ".agents"] {
        let p = root.join(sub);
        if p.exists() {
            walk(&p);
        }
    }
}

#[test]
fn s8_docs_and_install_all_8_hosts_and_update_delegation() {
    let lope = env!("CARGO_BIN_EXE_lope");
    let repo = repo_root();

    // 1. FR39: `lope docs` outputs Lope v2 reference natively and is NOT shadowed by ~/.lope/docs/reference.md
    let shadow_tmp = tempfile::tempdir().unwrap();
    let legacy_docs = shadow_tmp.path().join(".lope/docs");
    fs::create_dir_all(&legacy_docs).unwrap();
    fs::write(legacy_docs.join("reference.md"), "traylinx/lope legacy 0.17.3 Zero Python dependencies").unwrap();
    let docs_out = Command::new(lope)
        .env("HOME", shadow_tmp.path())
        .env_remove("LOPE_DOCS_PATH")
        .env_remove("LOPE_SOURCE_DIR")
        .env("LOPE_PYTHON", "/nonexistent/python")
        .arg("docs")
        .output()
        .unwrap();
    assert!(docs_out.status.success(), "lope docs failed");
    let docs_txt = String::from_utf8_lossy(&docs_out.stdout);
    assert!(docs_txt.len() > 10_000, "docs output unexpectedly short");
    for needle in [
        "lope",
        env!("CARGO_PKG_VERSION"),
        "makakoo/lope",
        "lope-py",
        "LOPE_FORCE_PYTHON",
        "v2/config.toml",
        "lope --version",
    ] {
        assert!(docs_txt.contains(needle), "docs output missing {needle:?}");
    }
    for forbidden in ["traylinx/lope", "Zero Python dependencies", "PYTHONPATH=", "python3 -m lope", "super-lope"] {
        assert!(
            !docs_txt.to_lowercase().contains(&forbidden.to_lowercase()),
            "docs output still contains stale {forbidden:?}"
        );
    }
    let override_doc = shadow_tmp.path().join("custom-ref.md");
    fs::write(&override_doc, "# Custom Override Docs\n").unwrap();
    let docs_override = Command::new(lope)
        .env("HOME", shadow_tmp.path())
        .env("LOPE_DOCS_PATH", &override_doc)
        .env("LOPE_PYTHON", "/nonexistent/python")
        .arg("docs")
        .output()
        .unwrap();
    assert!(docs_override.status.success());
    assert!(String::from_utf8_lossy(&docs_override.stdout).contains("# Custom Override Docs"));

    // 2a. FR37: `lope install --host <h>` produces artifacts in a fresh HOME for all 8 hosts
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    prep_optional_host_dirs(home);
    for host in ["claude", "codex", "gemini", "opencode", "cursor", "vibe", "qwen", "pi"] {
        let out = Command::new(lope)
            .env("HOME", home)
            .env("LOPE_SOURCE_DIR", &repo)
            .args(["install", "--host", host])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "lope install --host {host} failed:\nstdout={stdout}\nstderr={stderr}");
        let expected_banner = format!("Lope v{}", env!("CARGO_PKG_VERSION"));
        assert!(stdout.contains(&expected_banner), "missing {expected_banner} banner for {host}:\n{stdout}");
        assert!(
            !stdout.contains("WARNING: python3 import check failed"),
            "unexpected python3 warning in packaged install for {host}:\n{stdout}"
        );
    }
    assert!(home.join(".claude/skills/lope").exists());
    assert!(home.join(".codex/skills/lope").exists());
    assert!(home.join(".gemini/commands/lope").exists());
    assert!(home.join(".config/opencode/commands/lope.md").exists());
    assert!(home.join(".cursor/agents/lope.md").exists());
    assert!(home.join(".vibe/skills/lope").exists());
    assert!(home.join(".qwen/skills/lope").exists());
    assert!(home.join(".agents/skills/lope").exists());

    // 2b. FR37: `--host all` matches `bash <repo>/install --host all`, is idempotent, and ships v2 skills
    let tmp_rust = tempfile::tempdir().unwrap();
    let tmp_bash = tempfile::tempdir().unwrap();
    let home_rust = tmp_rust.path();
    let home_bash = tmp_bash.path();
    prep_optional_host_dirs(home_rust);
    prep_optional_host_dirs(home_bash);

    let out_rust = Command::new(lope)
        .env("HOME", home_rust)
        .env("LOPE_SOURCE_DIR", &repo)
        .args(["install", "--host", "all"])
        .output()
        .unwrap();
    assert!(
        out_rust.status.success(),
        "lope install --host all failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out_rust.stdout),
        String::from_utf8_lossy(&out_rust.stderr)
    );
    let rust_stdout = String::from_utf8_lossy(&out_rust.stdout);
    assert!(!rust_stdout.contains("python3 -m lope"), "packaged install stdout leaked python3 -m lope:\n{rust_stdout}");

    let out_bash =
        Command::new("bash").arg(repo.join("install")).args(["--host", "all"]).env("HOME", home_bash).output().unwrap();
    assert!(
        out_bash.status.success(),
        "bash install --host all failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out_bash.stdout),
        String::from_utf8_lossy(&out_bash.stderr)
    );

    let layout_rust = collect_host_layout(home_rust);
    let layout_bash = collect_host_layout(home_bash);
    assert_eq!(layout_rust, layout_bash, "`lope install --host all` layout diverged from `bash install --host all`");

    // Second run in same HOME must remain idempotent across all 8 host trees.
    let out_rust_2 = Command::new(lope)
        .env("HOME", home_rust)
        .env("LOPE_SOURCE_DIR", &repo)
        .args(["install", "--host", "all"])
        .output()
        .unwrap();
    assert!(
        out_rust_2.status.success(),
        "second `lope install --host all` failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out_rust_2.stdout),
        String::from_utf8_lossy(&out_rust_2.stderr)
    );
    assert_eq!(
        collect_host_layout(home_rust),
        layout_rust,
        "second `lope install --host all` altered installed host layout"
    );
    scan_no_python_m_lope(home_rust);

    // 3. FR38 (OQ-2): `update` and `upgrade` remain delegated to Python
    for verb in ["update", "upgrade"] {
        let out = Command::new(lope)
            .env("LOPE_PYTHON", "/nonexistent/python")
            .env("LOPE_ROUTER_TRACE", "1")
            .arg(verb)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(127));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("delegate (NoCoverage)"), "{verb}: {err}");
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    use std::os::unix::fs::{symlink, PermissionsExt};

    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let ft = entry.file_type().unwrap();
        if ft.is_symlink() {
            let target = fs::read_link(&src_path).unwrap();
            symlink(target, &dst_path).unwrap();
        } else if ft.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).unwrap();
            let mode = fs::symlink_metadata(&src_path).unwrap().permissions().mode();
            fs::set_permissions(&dst_path, fs::Permissions::from_mode(mode)).unwrap();
        }
    }
}

#[test]
fn s8_packaged_and_symlinked_install_offline() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let lope = env!("CARGO_BIN_EXE_lope");
    let repo = repo_root();
    let root = tempfile::tempdir().unwrap();
    let prefix = root.path().join("prefix with spaces");
    let share = prefix.join("share/lope");
    let bin_dir = prefix.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&share).unwrap();

    let installed_bin = bin_dir.join("lope");
    fs::copy(lope, &installed_bin).unwrap();
    fs::set_permissions(&installed_bin, fs::Permissions::from_mode(0o755)).unwrap();

    fs::copy(repo.join("install"), share.join("install")).unwrap();
    fs::set_permissions(share.join("install"), fs::Permissions::from_mode(0o755)).unwrap();
    copy_dir_recursive(&repo.join("skills"), &share.join("skills"));
    copy_dir_recursive(&repo.join("commands"), &share.join("commands"));

    let sym_dir = root.path().join("sym bin");
    fs::create_dir_all(&sym_dir).unwrap();
    let sym_bin = sym_dir.join("lope");
    symlink(&installed_bin, &sym_bin).unwrap();

    let home = root.path().join("isolated home");
    fs::create_dir_all(&home).unwrap();

    let out = Command::new(&sym_bin)
        .current_dir(std::env::temp_dir())
        .env("HOME", &home)
        .env("LOPE_HOME", home.join(".lope"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("PATH", "/usr/bin:/bin")
        .env_remove("LOPE_SOURCE_DIR")
        .env_remove("LOPE_BIN")
        .args(["install", "--host", "codex"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "symlinked packaged install failed:\nstdout={stdout}\nstderr={stderr}");
    assert!(
        stdout.contains(&format!("OK — lope {}", env!("CARGO_PKG_VERSION"))),
        "expected verified v2 version output:\n{stdout}"
    );
    assert!(
        stdout.contains(&share.join("skills/lope").display().to_string()),
        "expected packaged share/lope skills path, got:\n{stdout}"
    );
    assert!(home.join(".codex/skills/lope/SKILL.md").is_file());
    assert!(!stdout.contains("python3 -m lope"));

    let docs = Command::new(&sym_bin)
        .current_dir(std::env::temp_dir())
        .env("HOME", &home)
        .env_remove("LOPE_SOURCE_DIR")
        .env_remove("LOPE_DOCS_PATH")
        .arg("docs")
        .output()
        .unwrap();
    assert!(docs.status.success());
    assert!(String::from_utf8_lossy(&docs.stdout).contains(env!("CARGO_PKG_VERSION")));
}
