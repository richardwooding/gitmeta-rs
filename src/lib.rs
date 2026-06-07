//! Fast **per-file git metadata** for Rust — last-commit time / author /
//! subject, first-seen, commit count (churn), and tracked / ignored
//! status — resolved by scanning a working tree **once** and answering
//! per-path lookups in constant time.
//!
//! The batch design is the point: one [`Cache`] runs `git ls-files` plus a
//! single `git log` pass up front, so a 10k-file / 5k-commit repo costs a
//! handful of git invocations (~½ s) instead of 10k `git log -1 -- <path>`
//! calls (~100 s). It shells out to the system `git` binary rather than
//! reimplementing git.
//!
//! # One-shot [`Cache`]
//!
//! ```no_run
//! # fn main() -> Result<(), gitmeta::Error> {
//! let Some(cache) = gitmeta::Cache::new("/path/to/repo")? else {
//!     // Not a git working tree (or no git binary) — treat as "no git data".
//!     return Ok(());
//! };
//!
//! if let Some(info) = cache.lookup("/path/to/repo/src/main.rs") {
//!     println!("{} by {} — {} commits",
//!         info.last_commit_time, info.last_commit_author, info.commit_count);
//! }
//! assert!(cache.is_tracked("/path/to/repo/Cargo.toml") || true);
//! # Ok(()) }
//! ```
//!
//! # [`Pool`] — reuse across calls
//!
//! A [`Pool`] keeps one [`Cache`] per repo and re-validates on HEAD
//! change, so repeated lookups over an unchanging tree don't re-scan —
//! ideal for a long-running process (server, watcher, language tooling).
//!
//! ```no_run
//! # fn main() -> Result<(), gitmeta::Error> {
//! let pool = gitmeta::Pool::new();
//! if let Some(cache) = pool.get("/path/to/repo")? {
//!     let _ = cache.is_tracked("/path/to/repo/README.md");
//! }
//! # Ok(()) }
//! ```
//!
//! # Why git rather than filesystem mtimes?
//!
//! A fresh clone sets every file's mtime to checkout time — so "recently
//! changed" / "hot file" questions need git history, not the filesystem.
//!
//! # Async
//!
//! Enable the `tokio` feature for [`Cache::new_async`],
//! [`Pool::get_async`], and [`Pool::warm_async`]. The sync API pulls in no
//! async runtime.

#![forbid(unsafe_code)]

mod cache;
mod error;
mod info;
mod parse;
mod pool;
mod runner;

#[cfg(feature = "tokio")]
mod async_api;

pub use cache::Cache;
pub use error::Error;
pub use info::FileGitInfo;
pub use pool::Pool;

use std::path::Path;

/// Whether a usable `git` executable is on `PATH`.
///
/// A cheap `PATH` scan (no subprocess). Useful for callers that want to
/// warn up front rather than silently produce empty metadata: when git is
/// absent, [`Cache::new`] returns `Ok(None)`.
pub fn has_git_binary() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: &[&str] = if cfg!(windows) {
        &["git.exe", "git.cmd", "git"]
    } else {
        &["git"]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|name| is_executable(&dir.join(name))))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
