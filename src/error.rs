//! Error type for hard git failures.
//!
//! Note the [`crate::Cache`] contract: a path that simply isn't a git
//! working tree (or a missing `git` binary) is *not* an error — it is
//! reported as `Ok(None)`. [`Error`] is reserved for a git that is
//! present but failing on the happy path (after `HEAD` is confirmed).

/// Errors returned when git is present but a command fails.
///
/// The "not a git tree" / "git absent" cases never produce an `Error`;
/// they surface as `Ok(None)` from [`Cache::new`](crate::Cache::new) and
/// [`Pool::get`](crate::Pool::get).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The `git` process could not be spawned (and the failure happened
    /// somewhere other than the initial repository probe, where a spawn
    /// failure is treated as "no git data").
    #[error("gitmeta: failed to spawn git: {0}")]
    Spawn(#[source] std::io::Error),

    /// A git subprocess exited with a non-zero status on the happy path
    /// (e.g. `ls-files` or `log` after `HEAD` was confirmed). Carries the
    /// invoked argument list, the exit status, and trimmed stderr for
    /// diagnosability.
    #[error("gitmeta: git {command} failed ({status}): {stderr}")]
    Git {
        /// The git subcommand and arguments that were run (sans the
        /// leading `-C <root>`).
        command: String,
        /// The process exit status, rendered (e.g. `exit status: 128`).
        status: String,
        /// Trimmed stderr from the failed invocation.
        stderr: String,
    },
}
