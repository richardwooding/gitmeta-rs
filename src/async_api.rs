//! Async (tokio) constructors, enabled by the `tokio` feature.
//!
//! These mirror the synchronous orchestration in [`crate::cache`] and
//! [`crate::pool`] exactly; only the subprocess spawning differs
//! ([`run_git_async`] vs `run_git_sync`). All parsing and assembly is the
//! shared pure core, so behavior is identical to the sync API.

use std::path::Path;
use std::sync::Arc;

use crate::cache::{
    alt_root, assemble, assemble_empty, trim_utf8, Cache, ARGS_HEAD, ARGS_LOG, ARGS_LS_IGNORED,
    ARGS_LS_TRACKED, ARGS_TOPLEVEL,
};
use crate::error::Error;
use crate::pool::Pool;
use crate::runner::run_git_async;

impl Cache {
    /// Async counterpart of [`Cache::new`]. Same `Ok(None)` /
    /// `Ok(Some)` / `Err` contract.
    ///
    /// Cancellation comes for free: dropping the returned future (e.g.
    /// via `tokio::time::timeout`) kills the in-flight git process.
    pub async fn new_async(root: impl AsRef<Path>) -> Result<Option<Cache>, Error> {
        let root = root.as_ref();

        let repo_root = match run_git_async(root, ARGS_TOPLEVEL).await {
            Ok(out) => trim_utf8(&out),
            Err(_) => return Ok(None),
        };
        if repo_root.is_empty() {
            return Ok(None);
        }
        let canonical = Path::new(&repo_root);
        let repo_root_alt = alt_root(root, &repo_root);

        let head_sha = match run_git_async(canonical, ARGS_HEAD).await {
            Ok(out) => trim_utf8(&out),
            Err(_) => {
                let tracked = match run_git_async(canonical, ARGS_LS_TRACKED).await {
                    Ok(out) => out,
                    Err(_) => return Ok(None),
                };
                let ignored = run_git_async(canonical, ARGS_LS_IGNORED)
                    .await
                    .unwrap_or_default();
                return Ok(Some(assemble_empty(repo_root, tracked, ignored)));
            }
        };

        let tracked = run_git_async(canonical, ARGS_LS_TRACKED).await?;
        let ignored = run_git_async(canonical, ARGS_LS_IGNORED).await?;
        let log = run_git_async(canonical, ARGS_LOG).await?;

        Ok(Some(assemble(
            repo_root,
            repo_root_alt,
            head_sha,
            tracked,
            ignored,
            log,
        )))
    }
}

impl Pool {
    /// Async counterpart of [`Pool::get`]. Preserves the `Arc` identity
    /// guarantee on hits and the tolerated first-get race on misses.
    pub async fn get_async(&self, root: impl AsRef<Path>) -> Result<Option<Arc<Cache>>, Error> {
        let root = root.as_ref();

        let canonical = match run_git_async(root, ARGS_TOPLEVEL).await {
            Ok(out) => trim_utf8(&out),
            Err(_) => return Ok(None),
        };
        if canonical.is_empty() {
            return Ok(None);
        }

        // snapshot drops the (sync) lock before we await, so no guard is
        // held across an await point.
        if let Some((cache, head_sha)) = self.snapshot(&canonical) {
            if let Ok(out) = run_git_async(Path::new(&canonical), ARGS_HEAD).await {
                if trim_utf8(&out) == head_sha {
                    return Ok(Some(cache));
                }
            }
        }

        let cache = match Cache::new_async(root).await? {
            Some(c) => Arc::new(c),
            None => return Ok(None),
        };
        let head_sha = cache.head_sha().to_string();
        self.store(canonical, Arc::clone(&cache), head_sha);
        Ok(Some(cache))
    }

    /// Async counterpart of [`Pool::warm`].
    pub async fn warm_async(&self, root: impl AsRef<Path>) -> Result<(), Error> {
        self.get_async(root).await.map(|_| ())
    }
}
