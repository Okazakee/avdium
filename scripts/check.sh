#!/usr/bin/env bash
#
# The repository's canonical check set.
#
# This is the single definition of what "the checks pass" means. `README.md`,
# `CONTRIBUTING.md`, `AGENTS.md` and the CI workflow all point here rather than
# restating the commands, so the four copies cannot drift apart.
#
# CI is the authority. Local hooks call this too, but a hook can be bypassed
# with `--no-verify`, so a green hook is feedback and a green CI run is proof.
#
# Usage: scripts/check.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

step() {
  echo
  echo "==> $*"
}

step "repository contract"
./scripts/check-repo.sh

step "cargo fmt --all --check"
cargo fmt --all --check

step "cargo clippy --locked --all-targets -- -D warnings"
cargo clippy --locked --all-targets -- -D warnings

step "cargo clippy --locked --all-targets --all-features -- -D warnings"
cargo clippy --locked --all-targets --all-features -- -D warnings

step "cargo test --locked --all-targets"
cargo test --locked --all-targets

step "cargo test --locked --all-targets --all-features"
cargo test --locked --all-targets --all-features

step "cargo doc --locked --all-features --no-deps (RUSTDOCFLAGS=-D warnings)"
RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps

echo
echo "check: ok"
