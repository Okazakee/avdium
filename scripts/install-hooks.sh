#!/usr/bin/env bash
#
# Opt this working copy into the repository's tracked hooks.
#
# `core.hooksPath` is local configuration and cannot be committed, so this
# script is how a contributor turns the hooks on. It writes **local** config
# only. It never touches global configuration and never touches shell startup
# files.
#
# The hooks are developer feedback. They can be bypassed with `--no-verify`, so
# CI remains the authority.
#
# Usage:
#   scripts/install-hooks.sh
#   scripts/install-hooks.sh --uninstall

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

HOOKS_DIR=".githooks"
current="$(git config --local --get core.hooksPath || true)"

if [ "${1:-}" = "--uninstall" ]; then
  if [ "$current" = "$HOOKS_DIR" ]; then
    git config --local --unset core.hooksPath
    echo "install-hooks: hooks uninstalled"
  else
    echo "install-hooks: nothing to uninstall (core.hooksPath is '${current:-unset}')"
  fi
  exit 0
fi

if [ ! -d "$HOOKS_DIR" ]; then
  echo "install-hooks: $HOOKS_DIR is missing" >&2
  exit 1
fi

# Refuse rather than silently replacing an existing hook setup.
if [ -n "$current" ] && [ "$current" != "$HOOKS_DIR" ]; then
  echo "install-hooks: core.hooksPath is already set to '$current'." >&2
  echo "install-hooks: refusing to replace it. If that is intended, unset it first:" >&2
  echo "install-hooks:   git config --local --unset core.hooksPath" >&2
  exit 1
fi

for hook in pre-commit pre-push; do
  if [ ! -f "$HOOKS_DIR/$hook" ]; then
    echo "install-hooks: $HOOKS_DIR/$hook is missing" >&2
    exit 1
  fi
  chmod +x "$HOOKS_DIR/$hook"
done

git config --local core.hooksPath "$HOOKS_DIR"

# Verify what git will actually use, rather than trusting the write.
configured="$(git config --local --get core.hooksPath)"
if [ "$configured" != "$HOOKS_DIR" ]; then
  echo "install-hooks: verification failed, git reports '$configured'" >&2
  exit 1
fi

echo "install-hooks: core.hooksPath = $configured"
echo "install-hooks: pre-commit runs the repository contract and rustfmt"
echo "install-hooks: pre-push runs the full check set"
