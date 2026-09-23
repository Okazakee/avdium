# Contributing to Avdium

## Toolchain

The toolchain is pinned in `rust-toolchain.toml` (Rust `1.98.0`). `rustup`
installs it automatically. `Cargo.lock` is committed; use `--locked` in checks.

## Checks

Run the full set before opening a change:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps
```

CI runs the same commands. Local hooks and CI should call the same underlying
commands to avoid drift.

## Tests

- Behavioral changes need deterministic tests in the same change.
- Regression fixes should include a test that fails before the fix.
- Assert meaningful behavior and invariants, not existence or truthiness.
- Ordinary tests use temporary paths and no network.
- Network and real-emulator tests are explicit opt-in or ignored integration
  tests. They must never run by default.
- Tests must not accidentally fall through to real `android`, `sdkmanager`,
  `avdmanager`, `emulator`, `adb` or `java` binaries. Use the fake tools in
  `tests/fixtures/bin`, which record arguments and environment.

## Never mutate real user state

Do not point tests or manual runs at a real `ANDROID_HOME` or `~/.android/avd`
in a way that can edit, wipe or delete a device. Use disposable AVDs with
unique names and dedicated data directories.

## Commits

Keep commits topic-sized and history linear. Do not commit, push, tag or
release without explicit authorization. Do not rewrite history or force-push.

## Style

- Rust edition 2024, ESM-style module layout, `rustfmt` defaults.
- Prefer the smallest correct change that fits the existing design.
- Remove dead code instead of commenting it out.
- Explain non-obvious dependencies with short comments.
- No em dashes in user-facing strings.
- Use direct, useful error messages rather than marketing language.

## Adding a dependency

Justify it. Prefer the standard library and native platform features first.
Record non-obvious choices in `docs/decisions/`. Pin forks by immutable commit.
