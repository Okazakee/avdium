# Contributing to Avdium

## Toolchain

The toolchain is pinned in `rust-toolchain.toml` (Rust `1.98.0`). `rustup`
installs it automatically. `Cargo.lock` is committed; use `--locked` in checks.

## Checks

Run the full set before opening a change:

```sh
./scripts/check.sh
```

That script is the single definition of the check set, and CI calls it, so the
commands are not restated here.

It starts with a tree scan. That scan exists because a developer home path once
shipped inside a captured tool fixture, and removing it required rewriting
history. It rejects an absolute home path, a secret-shaped filename, a
credential, a private key, and captured Android SDK license text.
`./scripts/check.sh --self-test` proves the scan can fail, by planting each
violation in a throwaway repository and asserting it is rejected, and by
asserting a clean tree still passes.

## Dependency advisories

CI runs `cargo deny check advisories` against `deny.toml`. Only advisories are
configured. License, ban and source policy was evaluated and deliberately
dropped: it meant auditing five licenses reached indirectly through egui's
bundled fonts, arboard's Windows dependencies and the rustls root certificate
bundle, which is more upkeep than a project this size should carry. `AGENTS.md`
rule 7 already covers the obligation that matters, that reused third-party code
keeps its original notices.

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

### Writing a fake tool

The integration fixture generates each fake tool as a self-contained script with
its scenario **inlined at fixture-creation time**, so the tool reads nothing at
run time. This is deliberate. An earlier version wrote a scenario file and had
the tool read it, and under parallel load that read intermittently returned only
the first line, producing an empty tool with a zero exit status. Inlining
removed the class of problem instead of retrying around it.

Only `out`, `err` and `exit` are supported by the generator, and an unsupported
directive is a hard error, so a fixture cannot silently do nothing. Unit tests
that need the richer directives use `tests/fixtures/bin/fake-tool` with a
scenario file, where there is no parallel-execution pressure.

If you add a directive, add it to `render_scenario_script` in
`tests/common/mod.rs`.

## Tests

- Behavioral changes need deterministic tests in the same change.
- Regression fixes should include a test that fails before the fix.
- Assert meaningful behavior and invariants, not existence or truthiness.
- Ordinary tests use temporary paths, fake tools and an empty `PATH`. That is
  structural rather than sandboxed: there is no network namespace, so the
  guarantee is that no test is written to reach the network, not that it is
  prevented from doing so.
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
