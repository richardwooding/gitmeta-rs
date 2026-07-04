# gitmeta

[![CI](https://github.com/richardwooding/gitmeta-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/richardwooding/gitmeta-rs/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Website:** [richardwooding.github.io/gitmeta-rs](https://richardwooding.github.io/gitmeta-rs/)

Fast **per-file git metadata** for Rust — last-commit time / author / subject, first-seen,
commit count (churn), and tracked / ignored status — resolved by scanning a working tree
**once** and answering per-path lookups in constant time. Shells out to the system `git`
binary rather than reimplementing git.

The batch design is the point: one `Cache` runs `git ls-files` + a single `git log` pass up
front, so a 10k-file / 5k-commit repo costs a handful of git invocations (~½ s) instead of
10k `git log -1 -- <path>` calls (~100 s).

> A Rust port of the Go [`gitmeta`](https://github.com/richardwooding/gitmeta) library.

## One-shot `Cache`

```rust
let Some(cache) = gitmeta::Cache::new("/path/to/repo")? else {
    // Not a git working tree (or no git binary) — treat as "no git data".
    return Ok(());
};

if let Some(info) = cache.lookup("/path/to/repo/src/main.rs") {
    println!("{} by {} — {} commits",
        info.last_commit_time, info.last_commit_author, info.commit_count);
}

cache.is_tracked("/path/to/repo/Cargo.toml"); // bool
cache.is_ignored("/path/to/repo/target/x");   // bool
```

`lookup` returns a `&FileGitInfo`:

```rust
pub struct FileGitInfo {
    pub last_commit_time: jiff::Timestamp,
    pub last_commit_author: String,
    pub last_commit_subject: String,
    pub first_seen: jiff::Timestamp,
    pub commit_count: u32, // churn proxy
}
```

`Cache::new` returns `Ok(None)` when the path isn't a git working tree (or no `git` binary is
present) — handle it as "no git data" rather than an error. `Err` is reserved for a git that
is present but failing on the happy path.

Why git rather than filesystem mtimes? A fresh clone sets every file's mtime to checkout
time — so "recently changed" / "hot file" questions need git history, not the filesystem.

## `Pool` — reuse across calls

A `Pool` keeps one `Cache` per repo and **re-validates on HEAD change**, so repeated lookups
over an unchanging tree don't re-scan. Ideal for a long-running process (server, watcher,
language tooling) answering many git-metadata queries. `Pool::get` hands back an
`Arc<Cache>`, shared unchanged across cache hits.

```rust
let pool = gitmeta::Pool::new();
if let Some(cache) = pool.get("/path/to/repo")? { // built once per repo, refreshed when HEAD moves
    let _ = cache.is_tracked("/path/to/repo/README.md");
}
```

## Async

Enable the `tokio` feature for async constructors — the sync API pulls in no async runtime:

```toml
[dependencies]
gitmeta = { version = "0.1", features = ["tokio"] }
```

```rust
let cache = gitmeta::Cache::new_async("/path/to/repo").await?;
let pool = gitmeta::Pool::new();
let cache = pool.get_async("/path/to/repo").await?;
```

Cancellation comes for free: dropping the future (e.g. via `tokio::time::timeout`) kills the
in-flight git process.

## Requirements

- **Rust 1.79+** (MSRV).
- The system **`git`** binary on `PATH` (`gitmeta::has_git_binary()` reports its presence;
  `Cache::new` returns `Ok(None)` when git is absent or the path isn't a working tree).

## Differences from the Go original

- Non-UTF-8 paths are decoded lossily (Go carried raw bytes). The crate passes
  `-c core.quotePath=false` to keep non-ASCII paths literal — fixing a latent bug in the Go
  `log` parse.
- The sync API has no cancellation (Go used `context.Context`); the async API cancels via
  future-drop.

## License

MIT — see [LICENSE](LICENSE).
