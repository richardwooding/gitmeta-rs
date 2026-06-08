#!/usr/bin/env bash
# Read-only release preflight for the gitmeta crate.
# Usage: preflight.sh [target-version]
# Run from the repo root. Exits non-zero if any gate fails.
set -uo pipefail

fail=0
note() { printf '   %s\n' "$*"; }
ok()   { printf '\033[32m✅\033[0m %s\n' "$*"; }
bad()  { printf '\033[31m❌\033[0m %s\n' "$*"; fail=1; }

echo "── git state ──"
branch="$(git rev-parse --abbrev-ref HEAD)"
if [ "$branch" = "main" ]; then ok "on main"; else note "on branch '$branch' (expected main before bumping)"; fi
if [ -z "$(git status --porcelain)" ]; then ok "working tree clean"; else bad "working tree dirty"; fi
git fetch --quiet origin main || true
if [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main 2>/dev/null)" ]; then
  ok "in sync with origin/main"
else
  note "HEAD is not exactly origin/main — pull/rebase before releasing"
fi

echo "── crate identity ──"
name="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')"
cur="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')"
ok "crate: $name v$cur"

echo "── rustfmt ──";              if cargo fmt --check; then ok rustfmt; else bad rustfmt; fi
echo "── clippy ──";               if cargo clippy --all-features --all-targets --locked -- -D warnings; then ok clippy; else bad clippy; fi
echo "── tests (default) ──";      if cargo test --locked; then ok "tests (default)"; else bad "tests (default)"; fi
echo "── tests (all features) ──"; if cargo test --all-features --locked; then ok "tests (all features)"; else bad "tests (all features)"; fi

echo "── MSRV ──"
msrv="$(grep -E '^rust-version' Cargo.toml | sed -E 's/.*"([0-9.]+)".*/\1/')"
if [ -n "$msrv" ] && rustup toolchain list 2>/dev/null | grep -q "^$msrv"; then
  if cargo +"$msrv" build --all-features --locked && cargo +"$msrv" test --all-features --locked; then
    ok "MSRV $msrv build + test"
  else
    bad "MSRV $msrv"
  fi
else
  note "MSRV toolchain $msrv not installed — CI covers it. Install: rustup toolchain install $msrv"
fi

echo "── publish dry-run ──"; if cargo publish --locked --dry-run; then ok "publish dry-run"; else bad "publish dry-run"; fi

target="${1:-}"
if [ -n "$target" ]; then
  echo "── crates.io availability ──"
  code="$(curl -s -o /dev/null -w '%{http_code}' -H "User-Agent: ${name}-release" \
    "https://crates.io/api/v1/crates/${name}/${target}")"
  if [ "$code" = "200" ]; then bad "version $target is already on crates.io"; else ok "version $target is free on crates.io"; fi
fi

echo
if [ "$fail" -eq 0 ]; then echo "PREFLIGHT: PASS"; else echo "PREFLIGHT: FAIL"; fi
exit "$fail"
