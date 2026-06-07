//! Shared test helpers: spin up throwaway git repos in temp dirs.
//!
//! Rust's built-in test harness has no `t.Skip`, so each test that needs
//! git calls [`init_repo`], which returns `None` when no git binary is
//! present; callers `return` early (a vacuous pass). CI always has git, so
//! coverage isn't lost.

#![allow(dead_code)] // helpers are shared across test binaries; not all use all.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Run `git -C <dir> <args>`, panicking on failure (test setup must
/// succeed once we've decided git is present).
pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("spawn git");
    assert!(
        status.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// Create a fresh git repo in a temp dir with a deterministic identity.
/// Returns `None` when the git binary isn't available (the skip signal).
pub fn init_repo() -> Option<TempDir> {
    if !gitmeta::has_git_binary() {
        eprintln!("git binary not on PATH; skipping git integration test");
        return None;
    }
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test User"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    Some(dir)
}

/// Write `content` to `relpath` under `root`, then `git add` + `git commit`
/// it with `msg`.
pub fn write_and_commit(root: &Path, relpath: &str, content: &str, msg: &str) {
    let full = root.join(relpath);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(&full, content).expect("write");
    git(root, &["add", relpath]);
    git(root, &["commit", "-q", "-m", msg]);
}
