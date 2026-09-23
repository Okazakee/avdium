# Avdium

A native Android virtual device manager.

Android emulators, without the IDE.

Avdium discovers or provisions an Android SDK, manages the emulator packages
and system images its users need, creates and configures AVDs, and starts and
stops them with actionable diagnostics. It is a desktop application, not an
IDE, and it does not require Android Studio or a system-wide Java install.

## Status

Avdium is at `0.1.0-dev`. It is under active development and has no stable
release. See `docs/STATUS.md` for what currently works and what is still
unverified.

## Scope in one paragraph

The first release targets Linux x86_64. It covers existing-SDK discovery,
configurable SDK and AVD locations, a setup path for required packages and a
chosen system image, package listing with selected install/update/remove,
AVD listing with supported creation, start, cold boot, stop, safe settings
edits, wipe and delete, plus device status, a bounded log viewer, a doctor and
a redacted diagnostic export. A demo mode renders synthetic devices without
touching a real SDK. Building APKs, Gradle, project scaffolding, arbitrary ADB
shells and IDE features are explicitly out of scope. `PRODUCT.md` holds the
full contract.

## No telemetry

Avdium has no telemetry and no hosted service. It also disables supported
telemetry in the tools it invokes where the tool exposes a supported opt-out.
This says nothing about the Android guest, which may make its own network
requests.

## Building

```sh
cargo build --release
cargo run
```

The toolchain is pinned in `rust-toolchain.toml`. `Cargo.lock` is committed.

## Checks

```sh
./scripts/check.sh
```

That is the canonical set, and it is what CI runs. It is defined once, in
`scripts/check.sh`, so this file, `CONTRIBUTING.md`, `AGENTS.md` and the CI
workflow cannot drift apart.

See `CONTRIBUTING.md` for the contribution workflow and `AGENTS.md` for
operational constraints.

## License

MIT. See `LICENSE`. Reused third-party code keeps its original notices.
Avdium is an independent project and is not affiliated with Google.
