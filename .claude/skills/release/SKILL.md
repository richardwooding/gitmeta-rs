---
name: release
description: Cut a release of the `gitmeta` crate — bump the version, run full local verification (fmt, clippy, tests, MSRV, publish dry-run), open a version-bump PR, then tag and let CI publish to crates.io. Invoke with /release [version | patch | minor | major].
argument-hint: [version | patch | minor | major]
disable-model-invocation: true
---

# Release the gitmeta crate

Drive a new release end to end, **pausing for confirmation before every
irreversible or outward-facing action** (pushing a branch, opening the PR,
tagging, publishing). The target version comes from the argument `$0` — an
explicit `X.Y.Z`, or `patch` / `minor` / `major` to bump the current version.
If `$0` is empty, ask the user for the target version before doing anything.

## Current state
- Working tree: !`git status --short || echo clean`
- Branch: !`git rev-parse --abbrev-ref HEAD`
- Current version: !`cargo metadata --no-deps --format-version 1 2>/dev/null | jq -r '.packages[0].version'`
- Latest on crates.io: !`curl -s -H 'User-Agent: gitmeta-release' https://crates.io/api/v1/crates/gitmeta | jq -r '.crate.max_version // "unpublished"'`

## Repo invariants (do not violate)

- **`main` is gated.** You cannot push to `main` or merge PRs directly — the user
  does that. Every change (including the version bump) goes through a branch and a
  PR the user merges.
- **Always pass `--locked`.** CI builds/tests/lints with `--locked`. Never run
  `cargo update` to bump dependencies. `getrandom` is pinned to 0.3.x so the Rust
  1.79 MSRV job keeps working (0.4 needs edition2024 / Cargo 1.85+).
- **MSRV is 1.79.** Verify with the 1.79 toolchain if it is installed; CI covers it
  either way.
- **Releases are tag-triggered.** Pushing a `vX.Y.Z` tag runs
  `.github/workflows/release.yml`, which publishes to crates.io via Trusted
  Publishing (OIDC). The workflow is idempotent — it skips if that version is
  already on crates.io.
- **Tag must equal the `Cargo.toml` version** (the workflow enforces this).
- **This repo commits with `richard.wooding@gmail.com`** (already set as the
  repo-local git email — do not use the work email).

## Process

### 1. Preflight (read-only) — on a clean, synced `main`
Resolve the target version first (from `$0`, else ask). Then run:

```bash
bash ${CLAUDE_SKILL_DIR}/scripts/preflight.sh <target-version>
```

It checks branch/clean/sync, runs fmt + clippy + tests (default & all-features),
the MSRV build (if 1.79 is installed), a `cargo publish --dry-run`, and confirms
`<target-version>` is not already on crates.io. **Stop if it reports `FAIL`.**

### 2. Bump the version
Edit the `version = "..."` line in `Cargo.toml` to the target, then sync the
lockfile entry for this package only (no dependency churn):

```bash
cargo check    # refreshes the gitmeta version in Cargo.lock; deps unchanged
```

Confirm `git diff` touches only `Cargo.toml` and the `gitmeta` entry in
`Cargo.lock`. If anything else moved in `Cargo.lock`, revert and investigate.

### 3. Branch + commit + PR — ⚠️ CONFIRM GATE
Show the user the diff and the planned version. On approval:

```bash
git checkout -b release/v<target-version>
git commit -am "release: v<target-version>"
git push -u origin release/v<target-version>
gh pr create --base main --title "release: v<target-version>" \
  --body "Version bump to v<target-version>. Merging + the v<target-version> tag will publish to crates.io."
```

Then wait for the PR's CI to pass: `gh pr checks <n> --watch`.

### 4. Hand off the merge
`main` is gated, so **ask the user to merge the PR** and wait until it is merged.
Then:

```bash
git checkout main && git pull --ff-only origin main
```

### 5. Tag + GitHub Release — ⚠️ CONFIRM GATE (this triggers the publish)
After the user confirms they want to publish:

```bash
gh release create v<target-version> --title "v<target-version>" --generate-notes
```

This creates the tag + Release and triggers `release.yml`.

### 6. Verify the publish
Watch the workflow and confirm the new version is live:

```bash
gh run watch "$(gh run list --workflow=release.yml -L1 --json databaseId -q '.[0].databaseId')"
curl -s -H 'User-Agent: gitmeta-release' \
  "https://crates.io/api/v1/crates/gitmeta/<target-version>" | jq -r '.version.num // .errors'
```

If the publish step fails with an auth error, Trusted Publishing is not yet
configured. Tell the user to set it up (crates.io → gitmeta → Settings → Trusted
Publishing: owner `richardwooding`, repo `gitmeta-rs`, workflow `release.yml`,
environment `release`) and re-run the workflow — or bootstrap once with a local
`cargo publish --locked`.

## Report
Summarize: the version released, the crates.io URL
(`https://crates.io/crates/gitmeta`), the GitHub Release URL, and the workflow
outcome.
