//! Pure parsing core, shared verbatim by the sync and async code paths.
//!
//! Nothing here performs I/O: every function takes already-captured git
//! output (or plain paths) and returns owned data. That keeps the
//! sync/async split confined to the thin orchestration in [`crate::cache`]
//! and [`crate::async_api`], and makes the tricky bits (the newest-first
//! log walk, the NUL terminator, the `..` rejection) unit-testable
//! without spawning git.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use jiff::Timestamp;

use crate::info::FileGitInfo;

/// Split NUL-delimited git output (from `-z` modes) into records,
/// discarding the trailing empty record git always emits after the final
/// path. Empty input yields an empty vec (not `[""]`).
///
/// Records are decoded with [`String::from_utf8_lossy`]: a non-UTF-8 path
/// becomes a best-effort lossy string. This differs from the Go original,
/// which carried raw bytes; such a mangled key won't match a UTF-8 lookup
/// path, so the file is effectively invisible rather than mismatched.
pub(crate) fn split_nul(bytes: &[u8]) -> Vec<String> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<String> = bytes
        .split(|&b| b == 0)
        .map(|rec| String::from_utf8_lossy(rec).into_owned())
        .collect();
    // Drop the trailing empty record produced by the NUL terminator.
    if parts.last().map(String::is_empty).unwrap_or(false) {
        parts.pop();
    }
    parts
}

/// Parse a `git log --name-only --no-renames
/// --format=COMMIT\t%H\t%at\t%an\t%s HEAD` stream.
///
/// Commits arrive newest-first (git's default order). For each path a
/// commit touches:
///   - the **first** sighting fixes `last_commit_{time,author,subject}`,
///   - **every** sighting overwrites `first_seen` (so the oldest commit
///     wins) and increments `commit_count`.
///
/// Malformed commit headers and unparseable `%at` values are skipped,
/// matching the Go original's defensive `continue`.
pub(crate) fn parse_log(out: &str) -> HashMap<String, FileGitInfo> {
    let mut files: HashMap<String, FileGitInfo> = HashMap::new();

    let mut cur_time = Timestamp::UNIX_EPOCH;
    let mut cur_author = String::new();
    let mut cur_subject = String::new();
    let mut have_commit = false;

    for line in out.split('\n') {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("COMMIT\t") {
            // [%H, %at, %an, %s] — the subject keeps any embedded tabs.
            let parts: Vec<&str> = rest.splitn(4, '\t').collect();
            if parts.len() < 4 {
                continue;
            }
            let secs: i64 = match parts[1].parse() {
                Ok(s) => s,
                Err(_) => continue,
            };
            let ts = match Timestamp::from_second(secs) {
                Ok(t) => t,
                Err(_) => continue,
            };
            cur_time = ts;
            cur_author = parts[2].to_string();
            cur_subject = parts[3].to_string();
            have_commit = true;
            continue;
        }
        if !have_commit {
            // A path before any commit header — shouldn't happen with the
            // format above, but be defensive.
            continue;
        }
        let entry = files
            .entry(line.to_string())
            .or_insert_with(|| FileGitInfo {
                last_commit_time: cur_time,
                last_commit_author: cur_author.clone(),
                last_commit_subject: cur_subject.clone(),
                first_seen: cur_time,
                commit_count: 0,
            });
        // Walking newest-first → keep overwriting first_seen with the
        // older value. Don't touch last_commit_*; it's frozen at first
        // sight.
        entry.first_seen = cur_time;
        entry.commit_count += 1;
    }

    files
}

/// Convert `abs` to a forward-slash repo-relative key under `base`, or
/// `None` when `abs` is not genuinely under `base`.
///
/// Uses [`Path::strip_prefix`], which is purely lexical and only succeeds
/// when `abs` sits under `base` — so the Go original's explicit `..`
/// rejection is implicit here. Callers must pass already-absolutized
/// paths (the prefix match won't resolve `.`/`..` components).
pub(crate) fn rel_under(base: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(base).ok()?;
    // git emits forward slashes on every platform; normalise the OS
    // separator so keys match across Windows callers.
    Some(
        rel.to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/"),
    )
}

/// Build a set from a vector of keys.
pub(crate) fn set_from(items: Vec<String>) -> HashSet<String> {
    items.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn split_nul_drops_trailing_empty() {
        assert_eq!(split_nul(b"a\0b\0c\0"), vec!["a", "b", "c"]);
    }

    #[test]
    fn split_nul_without_trailing_terminator() {
        assert_eq!(split_nul(b"a\0b"), vec!["a", "b"]);
    }

    #[test]
    fn split_nul_empty_is_empty() {
        assert!(split_nul(b"").is_empty());
    }

    #[test]
    fn parse_log_single_commit() {
        let out = "COMMIT\tabc\t1700000000\tAlice\tAdd hello\nhello.txt\n";
        let files = parse_log(out);
        let info = files.get("hello.txt").expect("hello.txt present");
        assert_eq!(info.commit_count, 1);
        assert_eq!(info.last_commit_author, "Alice");
        assert_eq!(info.last_commit_subject, "Add hello");
        // Single commit ⇒ first_seen == last_commit_time.
        assert_eq!(info.first_seen, info.last_commit_time);
    }

    #[test]
    fn parse_log_accumulates_newest_first() {
        // Newest commit first (1200), oldest last (1000).
        let out = "\
COMMIT\tc3\t1200\tBob\tFinal pass\ndoc.md\n\
\nCOMMIT\tc2\t1100\tBob\tEdit pass\ndoc.md\n\
\nCOMMIT\tc1\t1000\tAlice\tInitial draft\ndoc.md\n";
        let info = parse_log(out).remove("doc.md").expect("doc.md present");
        assert_eq!(info.commit_count, 3);
        assert_eq!(info.last_commit_subject, "Final pass");
        assert_eq!(info.last_commit_time, Timestamp::from_second(1200).unwrap());
        assert_eq!(info.first_seen, Timestamp::from_second(1000).unwrap());
        assert!(info.first_seen < info.last_commit_time);
    }

    #[test]
    fn parse_log_subject_with_tabs_preserved() {
        let out = "COMMIT\tabc\t1700000000\tAlice\twith\ttabs\nf.txt\n";
        let info = parse_log(out).remove("f.txt").unwrap();
        assert_eq!(info.last_commit_subject, "with\ttabs");
    }

    #[test]
    fn parse_log_skips_malformed_at() {
        // Bad %at ⇒ commit skipped; no file recorded.
        let out = "COMMIT\tabc\tnot-a-number\tAlice\tBroken\nf.txt\n";
        assert!(parse_log(out).is_empty());
    }

    #[test]
    fn rel_under_strips_prefix() {
        let base = PathBuf::from("/repo");
        let abs = PathBuf::from("/repo/src/main.rs");
        assert_eq!(rel_under(&base, &abs).as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn rel_under_rejects_outside() {
        let base = PathBuf::from("/repo");
        let abs = PathBuf::from("/other/main.rs");
        assert_eq!(rel_under(&base, &abs), None);
    }

    #[test]
    fn rel_under_rejects_sibling_prefix() {
        // /repo-other is NOT under /repo despite the string prefix.
        let base = PathBuf::from("/repo");
        let abs = PathBuf::from("/repo-other/main.rs");
        assert_eq!(rel_under(&base, &abs), None);
    }
}
