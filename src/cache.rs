//! The per-repository scan result and its synchronous constructor.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::Error;
use crate::info::FileGitInfo;
use crate::parse::{parse_log, rel_under, set_from, split_nul};
use crate::runner::run_git_sync;

// git argument lists, shared by the sync ([`Cache::new`]) and async
// ([`Cache::new_async`]) orchestration so the two paths stay identical
// apart from how the subprocess is spawned.
pub(crate) const ARGS_TOPLEVEL: &[&str] = &["rev-parse", "--show-toplevel"];
pub(crate) const ARGS_HEAD: &[&str] = &["rev-parse", "HEAD"];
pub(crate) const ARGS_LS_TRACKED: &[&str] = &["ls-files", "-z"];
pub(crate) const ARGS_LS_IGNORED: &[&str] = &[
    "ls-files",
    "--others",
    "--ignored",
    "--exclude-standard",
    "-z",
];
// `-c core.quotePath=false` keeps non-ASCII paths literal in the
// (non-`-z`) log output; the Go original omitted it and would mangle such
// paths. `--no-renames` makes a path's history a flat list of touches.
pub(crate) const ARGS_LOG: &[&str] = &[
    "-c",
    "core.quotePath=false",
    "log",
    "--name-only",
    "--no-renames",
    "--format=COMMIT\t%H\t%at\t%an\t%s",
    "HEAD",
];

/// A scanned working tree. Build one with [`Cache::new`] (or
/// [`Cache::new_async`] under the `tokio` feature) and consult it with
/// [`lookup`](Cache::lookup), [`is_tracked`](Cache::is_tracked), and
/// [`is_ignored`](Cache::is_ignored).
///
/// A `Cache` is immutable after construction and cheap to share (see
/// [`Pool`](crate::Pool), which hands out `Arc<Cache>`).
#[derive(Debug)]
pub struct Cache {
    /// git's canonical view of the working-tree root (`git rev-parse
    /// --show-toplevel`). On macOS this is the realpath form
    /// (`/private/tmp/...`), which can differ from a symlinked path a
    /// caller passes (`/tmp/...`).
    repo_root: String,
    /// The as-supplied root, absolutized, when it differs from
    /// `repo_root`. Used as a fallback prefix in [`Cache::to_rel`] so
    /// callers can pass symlinked paths without resolving them first.
    repo_root_alt: Option<String>,
    /// HEAD commit SHA at build time. Empty string for the empty-repo
    /// (no commits) case.
    head_sha: String,
    /// Keyed by repo-relative forward-slash path (the form `git ls-files`
    /// emits).
    files: HashMap<String, FileGitInfo>,
    tracked: HashSet<String>,
    ignored: HashSet<String>,
}

impl Cache {
    /// Scan the git working tree containing `root`.
    ///
    /// Returns `Ok(None)` when `root` is not inside any git working tree,
    /// or when the `git` binary isn't available — the silent-skip signal
    /// callers treat as "no git data; leave fields at their defaults".
    /// Returns `Err` only for hard failures of a *present* git on the
    /// happy path (e.g. `ls-files` / `log` failing after `HEAD` is
    /// confirmed).
    ///
    /// An empty repository (initialised, no commits) yields
    /// `Ok(Some(_))` with empty per-file metadata so
    /// [`is_tracked`](Cache::is_tracked) / [`is_ignored`](Cache::is_ignored)
    /// still answer.
    pub fn new(root: impl AsRef<Path>) -> Result<Option<Cache>, Error> {
        let root = root.as_ref();

        // Probe: any failure (not-a-repo, git absent, permission denied)
        // degrades to "no cache" rather than an error.
        let repo_root = match run_git_sync(root, ARGS_TOPLEVEL) {
            Ok(out) => trim_utf8(&out),
            Err(_) => return Ok(None),
        };
        if repo_root.is_empty() {
            return Ok(None);
        }
        let canonical = Path::new(&repo_root);
        let repo_root_alt = alt_root(root, &repo_root);

        // HEAD missing ⇒ empty repo. Still a valid tree; build a cache
        // with no per-file metadata so tracked/ignored work.
        let head_sha = match run_git_sync(canonical, ARGS_HEAD) {
            Ok(out) => trim_utf8(&out),
            Err(_) => {
                let tracked = match run_git_sync(canonical, ARGS_LS_TRACKED) {
                    Ok(out) => out,
                    Err(_) => return Ok(None),
                };
                let ignored = run_git_sync(canonical, ARGS_LS_IGNORED).unwrap_or_default();
                return Ok(Some(assemble_empty(repo_root, tracked, ignored)));
            }
        };

        let tracked = run_git_sync(canonical, ARGS_LS_TRACKED)?;
        let ignored = run_git_sync(canonical, ARGS_LS_IGNORED)?;
        let log = run_git_sync(canonical, ARGS_LOG)?;

        Ok(Some(assemble(
            repo_root,
            repo_root_alt,
            head_sha,
            tracked,
            ignored,
            log,
        )))
    }

    /// The repository's top-level absolute directory (`git rev-parse
    /// --show-toplevel`).
    pub fn repo_root(&self) -> &str {
        &self.repo_root
    }

    /// The HEAD commit SHA at build time (empty for an empty repo).
    pub fn head_sha(&self) -> &str {
        &self.head_sha
    }

    /// Git metadata for `abs_path`, or `None` when it isn't tracked in
    /// this working tree (untracked, ignored, or outside the repo).
    /// `abs_path` must be absolute.
    pub fn lookup(&self, abs_path: impl AsRef<Path>) -> Option<&FileGitInfo> {
        let rel = self.to_rel(abs_path.as_ref())?;
        self.files.get(&rel)
    }

    /// Whether `abs_path` is in git's index for this working tree.
    pub fn is_tracked(&self, abs_path: impl AsRef<Path>) -> bool {
        self.to_rel(abs_path.as_ref())
            .is_some_and(|rel| self.tracked.contains(&rel))
    }

    /// Whether `abs_path` is matched by a git ignore rule but not tracked.
    /// Tracked files are never reported as ignored, matching git's
    /// `check-ignore` semantics.
    pub fn is_ignored(&self, abs_path: impl AsRef<Path>) -> bool {
        self.to_rel(abs_path.as_ref())
            .is_some_and(|rel| self.ignored.contains(&rel))
    }

    /// Convert `abs_path` to a forward-slash repo-relative key. Tries
    /// `repo_root` (git's canonical view) then `repo_root_alt` (the
    /// as-supplied root) to cover the macOS `/tmp` ↔ `/private/tmp`
    /// symlink case without an `EvalSymlinks`-style stat per lookup.
    fn to_rel(&self, abs_path: &Path) -> Option<String> {
        if abs_path.as_os_str().is_empty() {
            return None;
        }
        if let Some(rel) = rel_under(Path::new(&self.repo_root), abs_path) {
            return Some(rel);
        }
        if let Some(alt) = &self.repo_root_alt {
            if let Some(rel) = rel_under(Path::new(alt), abs_path) {
                return Some(rel);
            }
        }
        None
    }
}

/// Trim and lossily decode captured git stdout.
pub(crate) fn trim_utf8(out: &[u8]) -> String {
    String::from_utf8_lossy(out).trim().to_string()
}

/// The user-supplied root absolutized, when it differs from git's
/// canonical view; `None` when they match. Lexical only (does not resolve
/// symlinks), which is exactly what makes the dual-root fallback useful.
pub(crate) fn alt_root(user_root: &Path, canonical: &str) -> Option<String> {
    let abs = std::path::absolute(user_root).ok()?;
    let abs = abs.to_string_lossy().into_owned();
    if abs == canonical {
        None
    } else {
        Some(abs)
    }
}

/// Assemble a full cache from the captured outputs of the happy path.
pub(crate) fn assemble(
    repo_root: String,
    repo_root_alt: Option<String>,
    head_sha: String,
    tracked_out: Vec<u8>,
    ignored_out: Vec<u8>,
    log_out: Vec<u8>,
) -> Cache {
    Cache {
        repo_root,
        repo_root_alt,
        head_sha,
        files: parse_log(&String::from_utf8_lossy(&log_out)),
        tracked: set_from(split_nul(&tracked_out)),
        ignored: set_from(split_nul(&ignored_out)),
    }
}

/// Assemble the empty-repo (no HEAD) cache: no per-file metadata, but the
/// index and ignore sets still answer. Mirrors the Go `buildEmptyHeadCache`,
/// including leaving `repo_root_alt` unset.
pub(crate) fn assemble_empty(
    repo_root: String,
    tracked_out: Vec<u8>,
    ignored_out: Vec<u8>,
) -> Cache {
    Cache {
        repo_root,
        repo_root_alt: None,
        head_sha: String::new(),
        files: HashMap::new(),
        tracked: set_from(split_nul(&tracked_out)),
        ignored: set_from(split_nul(&ignored_out)),
    }
}
