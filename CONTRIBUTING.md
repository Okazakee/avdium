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

## Test layers

There are three, and each one proves something the others cannot.

### Unit tests (`src/**`, `#[cfg(test)]`)

Pure logic against captured real output: INI preservation, path and settings
resolution, package identifiers, catalog parsing, command construction, state
derivation, planning, consent, extraction safety, process mechanics. No
network, temporary paths only. This is where most assertions live, and where a
parser change belongs.

### Integration tests (`tests/*.rs`)

The built binary as a black box, driven with fake tools and a disposable SDK
under a temporary directory. These cover argument parsing, environment-driven
resolution, provider detection and printed output together, which unit tests
deliberately do not. They must never reach a real SDK or the network.

`tests/common/mod.rs` builds the sandbox. Note that it puts an empty directory
on `PATH`, so no test can accidentally find a real `android`, `sdkmanager`,
`avdmanager`, `emulator`, `adb` or `java`.

### End-to-end tests (`tests/e2e_lifecycle.rs`)

Opt-in and ignored by default. They download a real 172 MiB archive, boot a real
emulator and need KVM. They operate on a disposable SDK built from symlinks to
the developer's read-only inputs, and they assert that the real SDK was not
modified.

```sh
AVDIUM_E2E=1 cargo test --test e2e_lifecycle -- --ignored --test-threads=1
```

Run them on a machine with no other emulator running, because they inspect the
ADB device list.

### Known fixture instability

Four integration tests are marked `#[ignore]` with a reason. They assert on
output produced by a fake tool executed as a subprocess, and under load that
subprocess intermittently produces nothing, in a way that is not yet isolated.
The cause is in the fixture, not in Avdium. Do not paper over it with retries:
either isolate the cause or leave them ignored and say so.

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
