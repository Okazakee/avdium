#!/usr/bin/env bash
#
# Repository contract checks.
#
# This is the deterministic, offline half of the repository's verification. It
# asserts the things a compiler cannot: that required files and scripts are
# still present, that no local machine state has been committed, that the
# workflows keep their least-privilege shape, and that documented version facts
# agree with `Cargo.toml`.
#
# Why this exists rather than a secret scanner:
#
# Avdium has no credentials. Every one of the 508 entries in `Cargo.lock` comes
# from crates.io, no secret-shaped file has ever been committed, and there is no
# hosted service, database or deploy token. A full-history secret scan would be
# machinery guarding a threat this project does not have. What did actually leak
# on the first push was a developer home path inside a captured tool fixture,
# which required rewriting history to remove. The "no local state" check below
# targets that class of leak instead, at no dependency cost.
#
# Usage:
#   scripts/check-repo.sh [--root DIR] [--only CHECK] [--self-test]
#
#   --root DIR     check a different tree (used by --self-test)
#   --only CHECK   run one check: files, scripts, no-local-state, workflows,
#                  version, whitespace
#   --self-test    prove the checks can fail, then exit

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ONLY=""
SELF_TEST=0
# Set by --self-test. Declared here rather than inside the function because the
# EXIT trap runs after that function has returned.
SCRATCH=""

# Fictional user names allowed to appear as absolute home paths. These are
# synthetic values used by the documentation and by the path tests. A real
# machine's home directory is not in this list, so a real path is still caught.
HOME_PLACEHOLDERS=' example u user someone demo test alice bob '

usage() {
  sed -n '2,32p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
  case "$1" in
    --root) ROOT="${2:?--root needs a directory}"; shift 2 ;;
    --only) ONLY="${2:?--only needs a check name}"; shift 2 ;;
    --self-test) SELF_TEST=1; shift ;;
    -h | --help) usage; exit 0 ;;
    *) echo "check-repo: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

PROBLEMS=()

problem() {
  PROBLEMS+=("$1")
}

report() {
  local count=${#PROBLEMS[@]}
  if [ "$count" -eq 0 ]; then
    echo "check-repo: ok"
    return 0
  fi
  echo "check-repo: $count problem(s)" >&2
  local entry
  for entry in "${PROBLEMS[@]}"; do
    echo "  - $entry" >&2
  done
  return 1
}

# ------------------------------------------------------------------ checks

check_required_files() {
  local file
  for file in \
    Cargo.toml \
    Cargo.lock \
    rust-toolchain.toml \
    LICENSE \
    README.md \
    AGENTS.md \
    CONTRIBUTING.md \
    PRODUCT.md \
    PACKAGING.md \
    SECURITY.md \
    deny.toml \
    native-packages.yaml \
    docs/STATUS.md \
    docs/VALIDATION.md \
    docs/ROADMAP.md \
    docs/RELEASE-NOTES.md \
    docs/decisions/0006-ci-conventions.md \
    .github/CODEOWNERS \
    .github/dependabot.yml \
    .github/workflows/ci.yml \
    .github/workflows/packaging.yml \
    .github/workflows/release.yml \
    packaging/applications/dev.okazakee.Avdium.desktop \
    packaging/icons/dev.okazakee.Avdium.svg \
    packaging/arch/avdium-bin/PKGBUILD.in \
    packaging/test-install.sh \
    scripts/check.sh \
    scripts/check-repo.sh \
    scripts/install-hooks.sh \
    .githooks/pre-commit \
    .githooks/pre-push
  do
    if [ ! -e "$ROOT/$file" ]; then
      problem "missing required file: $file"
    fi
  done
}

check_executables() {
  local file
  for file in \
    scripts/check.sh \
    scripts/check-repo.sh \
    scripts/install-hooks.sh \
    .githooks/pre-commit \
    .githooks/pre-push \
    packaging/test-install.sh
  do
    if [ ! -e "$ROOT/$file" ]; then
      continue
    fi
    if [ ! -x "$ROOT/$file" ]; then
      problem "not executable: $file"
    fi
  done
}

# The secret-scanning substitute: nothing that belongs to a machine, an
# account, or an upstream license capture may be committed.
check_no_local_state() {
  local root="$1"
  local file path base
  while IFS= read -r file; do
    if [ -z "$file" ]; then
      continue
    fi
    path="$root/$file"
    if [ ! -f "$path" ]; then
      continue
    fi

    # This script necessarily contains the patterns it searches for, so it
    # cannot scan itself. It contains no secrets.
    if [ "$file" = "scripts/check-repo.sh" ]; then
      continue
    fi

    base="$(basename "$file")"
    case "$base" in
      .env | .env.* | *.pem | *.key | *.p12 | *.jks | *.keystore | *.token)
        problem "secret-shaped file is tracked: $file"
        ;;
    esac

    # `-I` skips binaries, which cannot carry these patterns meaningfully.
    if ! grep -qI . "$path" 2>/dev/null; then
      continue
    fi

    if grep -qIE 'AKIA[0-9A-Z]{16}|ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{20,}' "$path" 2>/dev/null; then
      problem "credential-shaped content: $file"
    fi

    if grep -qIF -- '-----BEGIN' "$path" 2>/dev/null &&
      grep -qIF 'PRIVATE KEY' "$path" 2>/dev/null; then
      problem "private key material: $file"
    fi

    # AGENTS.md rule 6 forbids committing license text captures. A capture is
    # the header together with the body, so a test asserting on the header line
    # alone is not one.
    #
    # `tests/fixtures/catalog/` is exempt, and is not a license capture: it is a
    # faithful capture of Google's public `repository2-3.xml`, which embeds
    # license text in `<license>` elements by design, and the catalog parser has
    # to be tested against that shape. The consent module never reads it;
    # consent binds to the live text fetched at consent time.
    case "$file" in
      tests/fixtures/catalog/*) ;;
      *)
        if grep -qIF 'Android Software Development Kit License Agreement' "$path" 2>/dev/null &&
          grep -qIF 'legally binding contract between you and Google' "$path" 2>/dev/null; then
          problem "captured Android SDK license text: $file"
        fi
        ;;
    esac

    # The lookbehind keeps `/demo/home/.android` and the redaction prefix list in
    # `src/app.rs` from being read as real home directories.
    local hit user
    while IFS= read -r hit; do
      if [ -z "$hit" ]; then
        continue
      fi
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
  done < <(git -C "$root" ls-files)
}

check_workflows() {
  local workflow line
  for workflow in \
    .github/workflows/ci.yml \
    .github/workflows/packaging.yml \
    .github/workflows/release.yml
  do
    if [ ! -f "$ROOT/$workflow" ]; then
      continue
    fi

    if ! grep -qE '^permissions:' "$ROOT/$workflow"; then
      problem "no top-level permissions: in $workflow"
    fi

    if grep -qF 'pull_request_target' "$ROOT/$workflow"; then
      problem "pull_request_target is not allowed: $workflow"
    fi

    # `secrets: inherit` hands every repository secret to the callee, which for
    # the packaging workflow is a third party. Secrets are passed explicitly.
    if grep -qF 'secrets: inherit' "$ROOT/$workflow"; then
      problem "secrets: inherit is not allowed: $workflow"
    fi

    while IFS= read -r line; do
      case "$line" in
        *'uses: ./'*) continue ;;
      esac
      if ! printf '%s' "$line" | grep -qE 'uses: [^ ]+@[0-9a-f]{40}( +#.*)?$'; then
        problem "action is not pinned to a commit SHA in $workflow:$(printf '%s' "$line" | sed 's/^[[:space:]]*/ /')"
      fi
    done < <(grep -E '^[[:space:]]*uses:' "$ROOT/$workflow" || true)
  done

  # Publication must stay behind an explicit authorization input.
  if [ -f "$ROOT/.github/workflows/release.yml" ]; then
    if ! grep -qF 'publish' "$ROOT/.github/workflows/release.yml"; then
      problem "release.yml no longer carries its explicit publish authorization gate"
    fi
  fi
}

check_version_consistency() {
  local version doc
  version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)"
  if [ -z "$version" ]; then
    problem "could not read the version from Cargo.toml"
    return
  fi
  for doc in README.md docs/STATUS.md; do
    if [ ! -f "$ROOT/$doc" ]; then
      continue
    fi
    if ! grep -qF "$version" "$ROOT/$doc"; then
      problem "$doc does not mention the Cargo.toml version $version"
    fi
  done
}

check_whitespace() {
  if ! git -C "$ROOT" diff --check >/dev/null 2>&1; then
    problem "git diff --check reports whitespace errors or conflict markers"
  fi
  if ! git -C "$ROOT" diff --cached --check >/dev/null 2>&1; then
    problem "git diff --cached --check reports whitespace errors or conflict markers"
  fi
}

run_checks() {
  case "$ONLY" in
    '')
      check_required_files
      check_executables
      check_no_local_state "$ROOT"
      check_workflows
      check_version_consistency
      check_whitespace
      ;;
    files) check_required_files ;;
    scripts) check_executables ;;
    no-local-state) check_no_local_state "$ROOT" ;;
    workflows) check_workflows ;;
    version) check_version_consistency ;;
    whitespace) check_whitespace ;;
    *)
      echo "check-repo: unknown check: $ONLY" >&2
      exit 2
      ;;
  esac
}

# ---------------------------------------------------------------- self-test

# A check that cannot fail proves nothing. This builds throwaway repositories
# with known violations and asserts the checks reject each one, and that a
# clean tree still passes.
self_test() {
  local failures=0
  SCRATCH="$(mktemp -d)"
  trap 'rm -rf "${SCRATCH:-}"' EXIT

  plant() {
    mkdir -p "$1"
    git -C "$1" init -q
    git -C "$1" config user.email "self-test@example.invalid"
    git -C "$1" config user.name "self-test"
  }

  stage() {
    git -C "$1" add -A
  }

  expect_pass() {
    if "$0" --root "$2" --only no-local-state >/dev/null 2>&1; then
      echo "  accepts $1"
    else
      echo "  FAILED: $1 was rejected" >&2
      failures=$((failures + 1))
    fi
  }

  expect_fail() {
    if "$0" --root "$2" --only no-local-state >/dev/null 2>&1; then
      echo "  FAILED: $1 was not detected" >&2
      failures=$((failures + 1))
    else
      echo "  detects $1"
    fi
  }

  # The canary name must not be an allowed placeholder, or the check could not
  # catch it and this self-test would pass for the wrong reason. Adding a real
  # user name to HOME_PLACEHOLDERS therefore breaks the self-test loudly.
  local canary_user='canary-user'
  case "$HOME_PLACEHOLDERS" in
    *" $canary_user "*)
      echo "  FAILED: the canary user name is in HOME_PLACEHOLDERS" >&2
      failures=$((failures + 1))
      ;;
  esac

  local clean="$SCRATCH/clean"
  plant "$clean"
  printf 'version = "0.0.0"\n' >"$clean/Cargo.toml"
  stage "$clean"
  expect_pass "a clean tree" "$clean"

  # The license header on its own is a test assertion, not a capture. This case
  # exists so the rule cannot silently widen into flagging assertions.
  local header="$SCRATCH/header-only"
  plant "$header"
  printf 'Android Software Development Kit License Agreement\n' >"$header/notes.txt"
  stage "$header"
  expect_pass "the license header alone" "$header"

  local name tree
  for name in home-path env-file credential private-key license-text; do
    tree="$SCRATCH/$name"
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
    stage "$tree"
    expect_fail "$name" "$tree"
  done

  if [ "$failures" -ne 0 ]; then
    echo "check-repo: self-test failed ($failures)" >&2
    return 1
  fi
  echo "check-repo: self-test passed"
  return 0
}

if [ "$SELF_TEST" -eq 1 ]; then
  self_test
  exit $?
fi

run_checks
if report; then
  exit 0
else
  exit 1
fi
