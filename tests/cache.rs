//! Integration tests for [`gitmeta::Cache`], ported from the Go suite.

mod common;

use std::thread::sleep;
use std::time::Duration;

use common::{init_repo, write_and_commit};
use gitmeta::Cache;

#[test]
fn new_single_commit_populates_lookup() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "hello.txt", "hi\n", "Add hello");

    let cache = Cache::new(root)
        .expect("New")
        .expect("cache for a real repo");
    assert!(!cache.repo_root().is_empty(), "repo_root empty");
    assert!(!cache.head_sha().is_empty(), "head_sha empty");

    let info = cache
        .lookup(root.join("hello.txt"))
        .expect("lookup hello.txt");
    assert_eq!(info.commit_count, 1);
    assert_eq!(info.last_commit_author, "Test User");
    assert_eq!(info.last_commit_subject, "Add hello");
    // Single commit ⇒ first_seen == last_commit_time.
    assert_eq!(info.first_seen, info.last_commit_time);
}

#[test]
fn new_multiple_commits_accumulate() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "doc.md", "v1\n", "Initial draft");
    // Sleep so commit timestamps are distinct at 1-second granularity.
    sleep(Duration::from_millis(1100));
    write_and_commit(root, "doc.md", "v2\n", "Edit pass");
    sleep(Duration::from_millis(1100));
    write_and_commit(root, "doc.md", "v3\n", "Final pass");

    let cache = Cache::new(root).expect("New").expect("cache");
    let info = cache.lookup(root.join("doc.md")).expect("lookup doc.md");
    assert_eq!(info.commit_count, 3);
    assert_eq!(info.last_commit_subject, "Final pass");
    assert!(
        info.first_seen < info.last_commit_time,
        "first_seen {} should be before last_commit_time {}",
        info.first_seen,
        info.last_commit_time
    );
}

#[test]
fn new_non_git_tree_returns_none() {
    // A plain temp dir (no git init). Holds even without a git binary.
    let dir = tempfile::TempDir::new().expect("tempdir");
    let cache = Cache::new(dir.path()).expect("nil err for non-git tree");
    assert!(cache.is_none(), "expected None for non-git tree");
}

#[test]
fn is_tracked_only_for_indexed_files() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "tracked.txt", "in\n", "Add tracked");
    std::fs::write(root.join("untracked.txt"), "out\n").expect("write untracked");

    let cache = Cache::new(root).expect("New").expect("cache");
    assert!(cache.is_tracked(root.join("tracked.txt")));
    assert!(!cache.is_tracked(root.join("untracked.txt")));
}

#[test]
fn is_ignored_matches_gitignore() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    std::fs::write(root.join(".gitignore"), "*.log\n").expect("write gitignore");
    common::git(root, &["add", ".gitignore"]);
    common::git(root, &["commit", "-q", "-m", "Add gitignore"]);
    std::fs::write(root.join("build.log"), "noise\n").expect("write log");

    let cache = Cache::new(root).expect("New").expect("cache");
    assert!(cache.is_ignored(root.join("build.log")));
    // .gitignore is tracked, so it's never reported as ignored.
    assert!(!cache.is_ignored(root.join(".gitignore")));
}

#[test]
fn lookup_path_outside_repo_returns_none() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "in.txt", "x\n", "Add");
    let cache = Cache::new(root).expect("New").expect("cache");

    let other = tempfile::TempDir::new().expect("tempdir");
    assert!(
        cache.lookup(other.path().join("in.txt")).is_none(),
        "lookup outside repo should be None"
    );
}
