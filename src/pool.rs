//! A process-lifetime cache of [`Cache`] instances, keyed by canonical
//! repository root and refreshed when HEAD changes.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::cache::{trim_utf8, Cache, ARGS_HEAD, ARGS_TOPLEVEL};
use crate::error::Error;
use crate::runner::run_git_sync;

/// A cached [`Cache`] paired with the HEAD SHA current when it was built.
struct PoolEntry {
    cache: Arc<Cache>,
    head_sha: String,
}

/// Caches one [`Cache`] per canonical repository root, re-validating on
/// HEAD change. Safe for concurrent use.
///
/// Designed to live for a long-running process (server, watcher, language
/// tooling) so repeated metadata queries against an unchanging tree share
/// a single scan. Every [`get`](Pool::get) runs one `git rev-parse HEAD`
/// (~3–5 ms) to check staleness, so a commit or checkout between calls is
/// picked up automatically.
#[derive(Default)]
pub struct Pool {
    entries: RwLock<HashMap<String, PoolEntry>>,
}

impl Pool {
    /// Create an empty pool. Caches are built lazily on first
    /// [`get`](Pool::get).
    pub fn new() -> Pool {
        Pool::default()
    }

    /// Return a [`Cache`] for the git tree containing `root`.
    ///
    /// On a hit (an entry exists and HEAD still matches) the cached
    /// `Arc<Cache>` is returned unchanged — `Arc::ptr_eq` holds across
    /// calls. Otherwise the cache is rebuilt via [`Cache::new`] and
    /// stored. Returns `Ok(None)` when `root` is not inside a git tree
    /// (same silent-skip contract as [`Cache::new`]); nothing is stored
    /// in that case.
    pub fn get(&self, root: impl AsRef<Path>) -> Result<Option<Arc<Cache>>, Error> {
        let root = root.as_ref();

        // Resolve the canonical root up front so we key the map by the
        // form that collapses /tmp and /private/tmp to one entry.
        let canonical = match run_git_sync(root, ARGS_TOPLEVEL) {
            Ok(out) => trim_utf8(&out),
            Err(_) => return Ok(None),
        };
        if canonical.is_empty() {
            return Ok(None);
        }

        // Hit fast path: snapshot the candidate + its HEAD, drop the lock,
        // then validate HEAD without holding it.
        if let Some((cache, head_sha)) = self.snapshot(&canonical) {
            if let Ok(out) = run_git_sync(Path::new(&canonical), ARGS_HEAD) {
                if trim_utf8(&out) == head_sha {
                    return Ok(Some(cache));
                }
            }
            // HEAD moved or rev-parse failed → fall through to rebuild.
        }

        // Miss / stale: rebuild. Pass the ORIGINAL root (not canonical) so
        // Cache's repo_root_alt fallback picks up the user-supplied
        // symlinked path.
        let cache = match Cache::new(root)? {
            Some(c) => Arc::new(c),
            None => return Ok(None), // race: canonical resolved but New saw nothing.
        };
        let head_sha = cache.head_sha().to_string();
        self.store(canonical, Arc::clone(&cache), head_sha);
        Ok(Some(cache))
    }

    /// Snapshot the cached `Arc<Cache>` + HEAD for `canonical`, dropping
    /// the read lock before returning. Keeps the (sync) lock off the
    /// async path's `.await` points.
    pub(crate) fn snapshot(&self, canonical: &str) -> Option<(Arc<Cache>, String)> {
        let entries = self.entries.read().expect("gitmeta pool lock poisoned");
        entries
            .get(canonical)
            .map(|e| (Arc::clone(&e.cache), e.head_sha.clone()))
    }

    /// Store (or replace) the entry for `canonical`.
    pub(crate) fn store(&self, canonical: String, cache: Arc<Cache>, head_sha: String) {
        let mut entries = self.entries.write().expect("gitmeta pool lock poisoned");
        entries.insert(canonical, PoolEntry { cache, head_sha });
    }

    /// Pre-build the cache for `root` (a [`get`](Pool::get) that discards
    /// the result), so the first real query doesn't pay the scan cost.
    pub fn warm(&self, root: impl AsRef<Path>) -> Result<(), Error> {
        self.get(root).map(|_| ())
    }

    /// The number of cached repositories.
    pub fn len(&self) -> usize {
        self.entries
            .read()
            .expect("gitmeta pool lock poisoned")
            .len()
    }

    /// Whether the pool holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
