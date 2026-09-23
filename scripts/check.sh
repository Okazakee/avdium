#!/usr/bin/env bash
#
# The repository's checks. One definition, called by CI and by hand.
#
# The tree scan runs first because it is instant, and because it catches the
# one class of mistake that is expensive to undo once pushed: a developer home
# path, a credential, or captured license text committed into a public
# repository. That happened once. A home path shipped inside a captured tool
# fixture and removing it required rewriting history.
#
# Usage:
#   scripts/check.sh              run everything
#   scripts/check.sh --self-test  prove the tree scan can actually fail

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Fictional user names allowed to appear as absolute home paths. These are
# synthetic values used by the documentation and by the path tests. A real
# machine's home directory is not in this list, so a real path is still caught.
HOME_PLACEHOLDERS=' example u user someone demo test alice bob '

PROBLEMS=()

problem() {
  PROBLEMS+=("$1")
}

# Nothing belonging to a machine, an account, or an upstream license capture
# may be committed.
scan_tree() {
  local file path base hit user
  while IFS= read -r file; do
    [ -n "$file" ] || continue
    path="$ROOT/$file"
    [ -f "$path" ] || continue

    # This script necessarily contains the patterns it searches for.
    [ "$file" = "scripts/check.sh" ] && continue

    base="$(basename "$file")"
    case "$base" in
      .env | .env.* | *.pem | *.key | *.p12 | *.jks | *.keystore | *.token)
        problem "secret-shaped file is tracked: $file"
        ;;
    esac

    # `-I` skips binaries, which cannot carry these patterns meaningfully.
    grep -qI . "$path" 2>/dev/null || continue

    if grep -qIE 'AKIA[0-9A-Z]{16}|ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{20,}' "$path" 2>/dev/null; then
      problem "credential-shaped content: $file"
    fi

    if grep -qIF -- '-----BEGIN' "$path" 2>/dev/null &&
      grep -qIF 'PRIVATE KEY' "$path" 2>/dev/null; then
      problem "private key material: $file"
    fi

    # AGENTS.md rule 6. A capture is the header plus the body, so a test that
    # asserts on the header line alone is not one. `tests/fixtures/catalog/` is
    # exempt and is not a license capture: it is a faithful capture of Google's
    # public `repository2-3.xml`, which embeds license text in `<license>`
    # elements by design, and the catalog parser is tested against that shape.
    case "$file" in
      tests/fixtures/catalog/*) ;;
      *)
        if grep -qIF 'Android Software Development Kit License Agreement' "$path" 2>/dev/null &&
          grep -qIF 'legally binding contract between you and Google' "$path" 2>/dev/null; then
          problem "captured Android SDK license text: $file"
        fi
        ;;
    esac

    # The lookbehind keeps `/demo/home/.android` and the redaction prefix list
    # in `src/app.rs` from being read as real home directories.
    while IFS= read -r hit; do
      [ -n "$hit" ] || continue
      user=""
      case "$hit" in
        /home/*) user="${hit#/home/}"; user="${user%%/*}" ;;
        /Users/*) user="${hit#/Users/}"; user="${user%%/*}" ;;
      esac
      case "$HOME_PLACEHOLDERS" in
        *" $user "*) continue ;;
      esac
      problem "absolute home path in $file: $hit"
    done < <(grep -oP '(?<![A-Za-z0-9._/-])(?:/home|/Users)/[A-Za-z0-9._-]+/' "$path" 2>/dev/null | sort -u || true)
  done < <(git ls-files)
}

report_tree() {
  local count=${#PROBLEMS[@]} entry
  if [ "$count" -eq 0 ]; then
    echo "tree scan: ok"
    return 0
  fi
  echo "tree scan: $count problem(s)" >&2
  for entry in "${PROBLEMS[@]}"; do
    echo "  - $entry" >&2
  done
  return 1
}

# A check that cannot fail proves nothing, so the scan is shown to reject each
# violation it claims to catch, and to accept a clean tree.
self_test() {
  local scratch failures=0 canary_user='canary-user'

  # If the canary name were an allowed placeholder, the scan could not catch it
  # and this self-test would pass for the wrong reason.
  case "$HOME_PLACEHOLDERS" in
    *" $canary_user "*)
      echo "self-test: FAILED, the canary user name is in HOME_PLACEHOLDERS" >&2
      return 1
      ;;
  esac

  scratch="$(mktemp -d)"
  trap 'rm -rf "${scratch:-}"' EXIT

  plant() {
    mkdir -p "$1"
    git -C "$1" init -q
    git -C "$1" config user.email "self-test@example.invalid"
    git -C "$1" config user.name "self-test"
  }

  expect_pass() {
    if "$0" --scan-only "$2" >/dev/null 2>&1; then
      echo "  accepts $1"
    else
      echo "  FAILED: $1 was rejected" >&2
      failures=$((failures + 1))
    fi
  }

  expect_fail() {
    if "$0" --scan-only "$2" >/dev/null 2>&1; then
      echo "  FAILED: $1 was not detected" >&2
      failures=$((failures + 1))
    else
      echo "  detects $1"
    fi
  }

  local clean="$scratch/clean"
  plant "$clean"
  printf 'version = "0.0.0"\n' >"$clean/Cargo.toml"
  git -C "$clean" add -A
  expect_pass "a clean tree" "$clean"

  # The license header on its own is a test assertion, not a capture. This case
  # stops the rule silently widening into flagging assertions.
  local header="$scratch/header-only"
  plant "$header"
  printf 'Android Software Development Kit License Agreement\n' >"$header/notes.txt"
  git -C "$header" add -A
  expect_pass "the license header alone" "$header"

  local name tree
  for name in home-path env-file credential private-key license-text; do
    tree="$scratch/$name"
    plant "$tree"
    case "$name" in
      home-path) printf 'sdk: /home/%s/Android/Sdk\n' "$canary_user" >"$tree/notes.txt" ;;
      env-file) printf 'TOKEN=placeholder\n' >"$tree/.env" ;;
      credential) printf 'token = "ghp_%s"\n' "$(printf 'a%.0s' $(seq 1 36))" >"$tree/notes.txt" ;;
      private-key) printf -- '-----BEGIN RSA PRIVATE KEY-----\n' >"$tree/notes.txt" ;;
      license-text)
        {
          printf 'Android Software Development Kit License Agreement\n\n'
          printf '1.1 The License Agreement forms a legally binding contract between you and Google.\n'
        } >"$tree/notes.txt"
        ;;
    esac
    git -C "$tree" add -A
    expect_fail "$name" "$tree"
  done

  if [ "$failures" -ne 0 ]; then
    echo "self-test: failed ($failures)" >&2
    return 1
  fi
  echo "self-test: passed"
  return 0
}

# ---------------------------------------------------------------- entry points

case "${1:-}" in
  --self-test)
    self_test
    exit $?
    ;;
  # Internal, used by --self-test to run the scan against a throwaway tree.
  --scan-only)
    ROOT="${2:?--scan-only needs a directory}"
    cd "$ROOT"
    scan_tree
    report_tree
    exit $?
    ;;
esac

step() {
  echo
  echo "==> $*"
}

step "tree scan"
scan_tree
report_tree

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
