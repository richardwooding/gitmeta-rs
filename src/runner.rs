//! The thin subprocess seam: run `git -C <root> <args>` and hand back
//! stdout bytes (or a typed [`Error`]).
//!
//! This is the *only* place the sync and async paths differ. Everything
//! downstream — parsing, set building, path resolution — is shared
//! ([`crate::parse`]). The sync runner uses [`std::process::Command`];
//! the async runner (behind the `tokio` feature) uses
//! [`tokio::process::Command`] with `kill_on_drop` so a dropped future
//! tears the git process down, matching the Go original's
//! `exec.CommandContext` cancellation.

use std::path::Path;
use std::process::Output;

use crate::error::Error;

/// Render the git invocation (sans the leading `-C <root>`) for error
/// messages.
fn command_str(args: &[&str]) -> String {
    args.join(" ")
}

/// Turn a finished process's [`Output`] into stdout bytes or an
/// [`Error::Git`] carrying the exit status and trimmed stderr.
fn map_output(args: &[&str], out: Output) -> Result<Vec<u8>, Error> {
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(Error::Git {
            command: command_str(args),
            status: out.status.to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

/// Run `git -C <root> <args>` synchronously, returning captured stdout.
///
/// A spawn failure becomes [`Error::Spawn`]; a non-zero exit becomes
/// [`Error::Git`]. Callers decide whether a given failure is fatal or a
/// silent "no git data" signal (the repository probe treats *any*
/// failure as the latter).
pub(crate) fn run_git_sync(root: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(Error::Spawn)?;
    map_output(args, out)
}

/// Async counterpart of [`run_git_sync`]. `kill_on_drop` ensures a
/// cancelled future (e.g. dropped by `tokio::time::timeout`) does not
/// leave an orphaned git process.
#[cfg(feature = "tokio")]
pub(crate) async fn run_git_async(root: &Path, args: &[&str]) -> Result<Vec<u8>, Error> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(Error::Spawn)?;
    map_output(args, out)
}
