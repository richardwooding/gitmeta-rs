//! Async integration tests, compiled only with the `tokio` feature.
#![cfg(feature = "tokio")]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{init_repo, write_and_commit};
use gitmeta::{Cache, Pool};

#[tokio::test]
async fn new_async_populates_lookup() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "hello.txt", "hi\n", "Add hello");

    let cache = Cache::new_async(root)
        .await
        .expect("new_async")
        .expect("cache");
    let info = cache.lookup(root.join("hello.txt")).expect("lookup");
    assert_eq!(info.commit_count, 1);
    assert_eq!(info.last_commit_subject, "Add hello");
    assert_eq!(info.first_seen, info.last_commit_time);
}

#[tokio::test]
async fn new_async_non_git_tree_returns_none() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let cache = Cache::new_async(dir.path()).await.expect("nil err");
    assert!(cache.is_none());
}

#[tokio::test]
async fn pool_get_async_reuses_cache() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "a.md", "# a\n", "Add a");
    let pool = Pool::new();

    let c1 = pool.get_async(root).await.expect("get").expect("cache");
    let c2 = pool.get_async(root).await.expect("get").expect("cache");
    assert!(Arc::ptr_eq(&c1, &c2), "reused cache for unchanged HEAD");
    assert_eq!(pool.len(), 1);
}

#[tokio::test]
async fn new_async_is_cancellable() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "a.md", "# a\n", "Add a");

    // A 1ns timeout drops the future almost immediately. We only assert
    // it returns promptly without hanging or panicking — kill_on_drop
    // tears down any in-flight git child. Either outcome is acceptable:
    // Ok(..) on a very fast machine, or Err(Elapsed).
    let res = tokio::time::timeout(Duration::from_nanos(1), Cache::new_async(root)).await;
    if let Ok(inner) = res {
        // If it actually completed, it must be a well-formed result.
        let _ = inner.expect("new_async ok");
    }
}
