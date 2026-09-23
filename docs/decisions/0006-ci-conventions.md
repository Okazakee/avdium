# Decision: verification, security and release conventions

Date: 2026-09-23
Status: accepted

## Context

Four sibling repositories were audited for conventions worth adopting:
`precall`, `pi-harness-config`, `minepanel-backend` and `blurkit`. Each was
strongest at a different layer, and none was worth copying wholesale.

Three problems in this repository motivated acting on the audit rather than
just recording it.

1. **A control that enforced nothing.** CI had a job named "Tests are offline"
   that set `AVDIUM_OFFLINE=1`. No file in `src/` or `tests/` read that
   variable. `CONTRIBUTING.md` and `docs/VALIDATION.md` both claimed the
   default run used no network, and nothing made that true.
2. **The same six commands in four places.** `README.md`, `CONTRIBUTING.md`,
   `AGENTS.md` and `.github/workflows/ci.yml` each restated the full check set.
   Four copies drift.
3. **A real output-loss defect.** `src/process.rs` decided a child had finished
   producing output by calling `receiver.try_recv().is_err()`. That call
   consumes and discards a queued chunk, and it cannot distinguish a
   momentarily empty channel from a closed one. Tool output was lost.

## Decision

**The check set has one definition.** `scripts/check.sh` is the canonical set,
and `scripts/check-repo.sh` is the repository contract. The four documents and
the CI workflow point at the script instead of restating commands.

**The contract is enforced, and its enforcement is tested.**
`scripts/check-repo.sh --self-test` plants a home path, a `.env`, a credential
and a license capture in throwaway repositories and asserts each is rejected,
and asserts a clean tree still passes. A check that cannot fail proves nothing,
so the self-test runs before the contract in CI.

**No secret scanning.** Avdium has no credentials: all 508 lock entries resolve
to crates.io, no secret-shaped file has ever been committed, and there is no
hosted service, database or deploy token. The only secrets that will ever exist
are AUR publish credentials, which live in GitHub's secret store. A pinned
TruffleHog binary, a per-platform checksum lockfile and a full-history scan
would be machinery guarding a threat this project does not have. What did
actually leak on the first push was a developer home path inside a captured
fixture, which required rewriting history to remove. The contract's
no-local-state check targets that class instead, at no dependency cost.

**The offline job is deleted, not propped up.** The claim was removed rather
than enforced. A network sandbox (`unshare -n`) would have been the enforcing
option, but the honest cheaper move is to state what is actually true: tests use
fake tools, temporary paths and an empty `PATH`, which is verifiable by reading
them.

**Dependency policy: advisories block, the rest is advisory.** 508 packages
make advisory scanning worthwhile. An all-blocking policy on day one would fail
on a license or duplicate-crate finding rather than on a real vulnerability, so
`licenses`, `bans` and `sources` run with `--warn` until reviewed.

**Hooks are feedback, CI is authority.** `.githooks/` is tracked and installed
with `core.hooksPath`, which is local configuration and cannot be committed.
`scripts/install-hooks.sh` writes local config only and refuses to replace a
different existing value. It never touches global configuration or shell startup
files.

## Rejected

- **`secrets: inherit`.** It hands every repository secret to the callee, and
  the packaging workflow forwards to a third-party reusable workflow. Secrets
  are now passed explicitly. `HOMEBREW_TAP_GITHUB_TOKEN` is no longer passed at
  all, because `targets` is `linux-amd64` only.
- **`cargo-audit`.** cargo-deny's advisory check reads the same RustSec
  database. Adding both is redundant machinery.
- **`CODEOWNERS` as a control.** It has no effect without branch protection
  requiring code-owner review, which is server-side. The file says so.
- **Commit-subject-driven automation**, which blurkit uses to gate a deploy. It
  is brittle under squash and merge rewrites.

## Consequences

`deny.toml` needed a real review rather than a copied allow-list. The first run
surfaced five licenses that had not been considered: `BSL-1.0` through
`arboard`'s Windows-only dependencies, `OFL-1.1` and `Ubuntu-font-1.0` through
egui's bundled fonts, `CDLA-Permissive-2.0` through the rustls root certificate
bundle, and `MPL-2.0` through `option-ext`. The first four are permissive. The
fifth is weak copyleft and is acceptable only because the crate is consumed
unmodified; if it is ever patched or vendored, the modified files must stay
`MPL-2.0`. That obligation is recorded in `deny.toml` itself.

`src/process.rs` was restructured rather than patched. Completion is now
signalled in band with a `PipeMessage::Finished` variant, `Partial` events are
display-only, and `finish()` retains a trailing line that has no newline. The
first of those is the defect fix; the other two follow from the same state
machine and are recorded in `docs/VALIDATION.md`.

The same review found that `extract_zip` never checked the symlink bit, while
`extract_tar_gz` rejects links outright and the module documentation claimed
both did. The ZIP path now rejects link entries too, so the two paths hold one
invariant and the documented claim is true rather than aspirational.

Tightening `licenses`, `bans` and `sources` to blocking is a one-line change in
`.github/workflows/ci.yml`. It is left advisory because that was the deliberate
choice, not because the tree fails: all four checks pass as written.
