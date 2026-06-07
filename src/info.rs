//! The per-file metadata record.

use jiff::Timestamp;

/// Per-file git metadata resolved for every tracked path in a working
/// tree.
///
/// The values are meaningful as a set: a freshly committed file has
/// `commit_count == 1` and `first_seen == last_commit_time`. As a file
/// accumulates commits, `last_commit_time` stays pinned to the newest
/// commit that touched it while `first_seen` walks back to the oldest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGitInfo {
    /// Author time of the most recent commit that touched this path.
    pub last_commit_time: Timestamp,
    /// Author of the most recent commit that touched this path.
    pub last_commit_author: String,
    /// Subject (first line of the message) of that most recent commit.
    pub last_commit_subject: String,
    /// Author time of the oldest commit that touched this path.
    pub first_seen: Timestamp,
    /// Number of commits that touched this path (a churn proxy).
    pub commit_count: u32,
}
