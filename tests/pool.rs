//! Integration tests for [`gitmeta::Pool`], ported from the Go suite.

mod common;

use std::sync::Arc;
use std::thread;

use common::{init_repo, write_and_commit};
use gitmeta::Pool;

#[test]
fn get_reuses_cache() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "a.md", "# a\n", "Add a");
    let pool = Pool::new();

    let c1 = pool.get(root).expect("first get").expect("cache");
    let c2 = pool.get(root).expect("second get").expect("cache");
    assert!(
        Arc::ptr_eq(&c1, &c2),
        "pool returned different caches for unchanged HEAD"
    );
    assert_eq!(pool.len(), 1);
}

#[test]
fn head_change_rebuilds() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "a.md", "# a\n", "Add a");
    let pool = Pool::new();

    let c1 = pool.get(root).expect("first get").expect("cache");
    let first_head = c1.head_sha().to_string();

    write_and_commit(root, "b.md", "# b\n", "Add b");

    let c2 = pool.get(root).expect("second get").expect("cache");
    assert!(
        !Arc::ptr_eq(&c1, &c2),
        "pool returned same cache after HEAD changed"
    );
    assert_ne!(c2.head_sha(), first_head, "cache HEAD did not update");
    assert!(
        c2.lookup(root.join("b.md")).is_some(),
        "post-commit cache should know about b.md"
    );
}

#[test]
fn concurrent_safe() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path().to_path_buf();
    write_and_commit(&root, "a.md", "# a\n", "Add a");
    let pool = Arc::new(Pool::new());

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let pool = Arc::clone(&pool);
            let root = root.clone();
            thread::spawn(move || pool.get(&root))
        })
        .collect();

    for h in handles {
        let cache = h.join().expect("thread panicked").expect("get");
        assert!(cache.is_some(), "got None cache");
    }
    // The first-get race may build duplicate caches, but the final map
    // must hold exactly one entry.
    assert_eq!(pool.len(), 1);
}

#[test]
fn non_git_tree_none() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let pool = Pool::new();
    let cache = pool.get(dir.path()).expect("nil err for non-git tree");
    assert!(cache.is_none());
    assert_eq!(pool.len(), 0, "no entry stored for non-git tree");
}

#[test]
fn warm_primes() {
    let Some(dir) = init_repo() else { return };
    let root = dir.path();
    write_and_commit(root, "a.md", "# a\n", "Add a");
    let pool = Pool::new();

    pool.warm(root).expect("warm");
    assert_eq!(pool.len(), 1);
    let cache = pool.get(root).expect("get after warm");
    assert!(cache.is_some());
}
