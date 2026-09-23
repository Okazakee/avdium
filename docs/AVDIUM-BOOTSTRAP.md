# Avdium: product specification and pi bootstrap brief

Date: 2026-09-23
Status: approved project name; implementation brief with explicit validation gates
Audience: the user's local pi coding harness

This document consolidates the original AVD-manager research and its independent review. It is self-contained and supersedes their conflicting implementation recommendations. Historical observations below remain observations until reproduced. Do not require the earlier documents to start work.

## 0. Start here, pi

Build **Avdium**, a native Android Virtual Device and SDK manager that works without Android Studio and does not require the user to install Java system-wide.

Start with the repository foundation, targeted toolchain validation, and a working manager for existing SDKs. Continue through the milestones in dependency order where access and evidence permit. Deliver working increments, not only more planning documents.

### Execution instructions

1. Read the actual applicable `AGENTS.md` files and installed pi harness instructions before editing. Follow the harness's current review, commit, and verification workflow. Do not infer its configuration from old OMP conventions or install/reconfigure the harness.
2. Inspect the current directory and Git state. Use an existing Avdium repository if this task was started inside one. Otherwise create `avdium/` beneath the user-selected workspace and initialize a local repository. Do not nest a repository accidentally, delete existing work, reset branches, or rewrite history.
3. Save this brief as `docs/AVDIUM-BOOTSTRAP.md`. Treat it as the initial product contract. Record implementation decisions and verified changes in short decision records rather than silently revising the evidence.
4. Build the foundation and M0 evidence in parallel where they are independent. A blocked network call or unavailable KVM must not prevent implementation of the UI shell, fake tools, parser tests, or existing-SDK discovery.
5. Finish M1 as the first useful implementation target. Then continue to M2 and subsequent milestones when their prerequisites are satisfied. Do not claim a milestone complete when a required real-world check remains unexecuted.
6. Make routine implementation decisions autonomously. Ask only when the answer materially affects product direction, access, new license acceptance, or an operation on existing user data that is not already authorized.
7. Use local topic-sized commits when allowed by the actual harness/repository rules. Prefer linear history. Existing branch protections and review requirements take precedence over the reference projects' habit of working directly on `main`.
8. This brief requests implementation and local verification. Prepare release automation and recipes, but do not publish packages, create a public repository, push release tags, or alter host privileges merely because a later milestone describes those actions.

### First-session outcome

- Avdium repository and product/contributor/agent instructions exist.
- A real egui application opens, including a useful no-SDK state.
- A demo mode renders synthetic devices with no access to the real SDK.
- Read-only discovery and doctor work against available local tools.
- Process infrastructure is exercised with fake tools, including cancellation.
- Toolchain findings, blockers, exact next steps, and executed checks are recorded.
- No real AVD has been edited, wiped, moved, or removed as a side effect of bootstrap.

If all prerequisites are available, go beyond this outcome and implement the next milestone. If a prerequisite is blocked, finish independent authorized work and report the specific remaining dependency.

## 1. Identity

| Item | Value |
| --- | --- |
| Product | **Avdium** |
| Repository, Cargo package, binary | `avdium` |
| Initial development version | `0.1.0-dev` |
| First stable release target | `0.1.0` |
| Description | A native Android virtual device manager. |
| Tagline | Android emulators, without the IDE. |
| Application identifier | `dev.okazakee.Avdium` |
| Desktop entry | `dev.okazakee.Avdium.desktop` |
| Installed icon name | `dev.okazakee.Avdium` |
| AUR binary package | `avdium-bin` |
| DEB/RPM package | `avdium` |
| Code license | MIT, preserving notices for reused code |

Use Avdium consistently in the window title, README, CLI help, diagnostics, packaging, and release filenames. No old candidate names or `<app>` placeholders in shipped files.

Do not advertise affiliation with Google. Use an original simple SVG icon. Do not use Android or Google branding as Avdium's logo.

Project-owned URLs must point to repositories/pages that actually exist. A planned `Okazakee/avdium` remote is not evidence that it has been created. Keep publication configuration inactive until its destination is established.

## 2. Product contract

### Problem

Developers using React Native, Expo, Flutter, or other workflows often need Android emulators without the rest of Android Studio. The official command-line tooling exists, but installation, image selection, device configuration, diagnostics, and lifecycle management require unnecessary manual work.

### Product

A small native desktop application that discovers or provisions an SDK, manages the emulator packages and images needed by its users, creates/configures AVDs, and starts/stops them with useful diagnostics.

The product's value is the complete setup and management experience. Existing standalone emulator launchers already exist, so a launcher-only interface is an intermediate milestone.

### Core principles

- No Android Studio, Gradle project, IDE, or account required to use Avdium.
- No system Java installation required. Providers may use a private runtime or an already-compatible Java installation.
- Standard SDK and AVD files remain interoperable with official tools.
- User-owned installation locations; no sudo during normal application operation.
- Existing installations are discovered and respected.
- User-selected downloads with integrity checks and explicit handling of required terms.
- Commands and operation output are inspectable. Execute argument arrays, never a reconstructed shell command.
- Actionable diagnostics for SDK paths, KVM, graphics, runtime dependencies, disk, and tool failures.
- Avdium itself has no telemetry or hosted service. Disable supported tool telemetry for Avdium-invoked operations and accurately describe third-party behavior.
- All blocking work stays off the UI thread.

### v0.1 release scope

- Linux x86_64.
- X11 and Wayland, with actual validation status documented separately.
- Existing SDK discovery and configurable SDK/AVD locations.
- Setup wizard for required packages and a selected system image.
- Package listing, selected install/update/remove operations with dependency checks.
- AVD listing, supported device/image creation, start, cold boot, stop, safe settings edits, wipe, and delete.
- Device status, bounded log viewer, doctor, and redacted diagnostic export.
- Demo mode and repeatable UI screenshots.
- Release tarball, DEB, RPM, and an AUR binary recipe using native-packages.

Creation with a user-selected image is a release requirement. If Android CLI cannot implement it, use a validated classic-tools creation adapter. Do not quietly substitute a generic profile with an unrelated API/image.

### Deferred scope

- Linux ARM64, macOS, and Windows support claims.
- APK building, Gradle, project creation, arbitrary ADB shell UI, and IDE functions.
- Waydroid, container Android, and a custom emulator engine.
- Full snapshot-management UI, full device-data cloning, SDK migration, and arbitrary disk resizing.
- Flatpak, AppImage, MSIX, DMG, Homebrew, i18n pipeline, tray integration, and application self-updating.
- Exhaustive parity with every flag of every official CLI.

Keep a feature/capability matrix. Do not use "full parity" as an undefined acceptance criterion. Basic ADB use for identity, readiness, and stopping is part of the core product, even though a general ADB frontend is out of scope.

## 3. Evidence baseline

### Independently verified during the preceding review

| Item | Evidence |
| --- | --- |
| `sdkmanager` deprecation | Current official documentation redirects users to `android sdk`. |
| `avdmanager` deprecation | Current official documentation also redirects users to Android CLI. |
| Android CLI launcher | Versioned Linux download fetched and hashed; exact details below. |
| Classic tools archive | Filename, byte size, and SHA-1 match Google's repository metadata. Full ZIP was not rehashed during review. |
| Emulator releases | Repository XML lists stable 37.1.11 and newer preview entries. Channel filtering is required. |
| Linux target | Inspected official emulator entries contain Linux x64 archives, not Linux aarch64 archives. |
| GUI reference stack | Upstream manifests confirm Rust edition 2024 and egui/eframe 0.36. |
| Packaging tools | ZapFast YAML pins native-packages 0.7.0 and nFPM 2.47.0. |

Do not infer product launch dates from documentation update dates. Do not equate a launcher hash with complete toolchain reproducibility.

### Historical host observations, not bootstrap prerequisites

The original research described a CachyOS laptop with `ANDROID_HOME=$HOME/Android/Sdk`, working KVM, Java 21.0.11, Rust 1.98.1, emulator 36.2.12, and three Studio-created devices: Pixel_9, Pixel_9_Pro, and Pixel_Tablet. It reported Android CLI version 1.0.16406183 listing those devices and SDK packages.

Re-probe the actual host. Do not assume these paths, versions, permissions, or devices still exist. Never hardcode them into application defaults or execute mutation tests against them.

The original research reported a CLI bundle with its own runtime, additional installation downloads, shared state under `.android`, and a separate installed application. These details require fresh observation before relying on their exact layout or behavior.

### Pinned inputs and unresolved inputs

| Artifact | Initial value | Verification status |
| --- | --- | --- |
| Android CLI version | `1.0.16406183` | Launcher identity verified |
| Launcher URL | `https://dl.google.com/android/cli/1.0.16406183/linux_x86_64/android` | Download verified |
| Launcher bytes | `5067072` | Verified |
| Launcher SHA-256 | `1e7f2da1bb678bdb3c78a20b421a276f6b9eb639f0b6a17d46fa02f01a0b67b4` | Verified |
| Classic tools revision | `23.0`, archive build `16111833` | Repository metadata verified |
| Classic ZIP URL | `https://dl.google.com/android/repository/commandlinetools-linux-16111833_latest.zip` | Metadata verified |
| Classic ZIP bytes | `181052239` | Metadata verified |
| Classic ZIP SHA-1 | `e025545c62a8e64c7559119566a569fb1dec5f60` | Google's metadata value |
| Classic ZIP SHA-256 | Not established | Resolve and record provenance before enforcing a SHA-256 lock |
| Managed Java | Temurin JRE 21 is the initial candidate | Resolve actual release, hash, and compatibility during M0 |
| Minimum Java for this classic-tools build | Not established | Execute the pinned tools against the selected runtime |

The previous JRE version/hash claim was not independently reproduced because the API request returned 403. Do not copy that unverified checksum into a trusted manifest.

Use the Adoptium assets API to resolve a specific release and its published checksum, then persist that exact selection. The binary endpoint `https://api.adoptium.net/v3/binary/latest/21/ga/linux/x64/jre/hotspot/normal/eclipse` is a discovery convenience, not an immutable lock.

## 4. Reference projects and stack

Study these repositories for patterns; Avdium is an independent project:

- `https://github.com/crmne/zapfast`, observed head `7e62788c587b811a451a3d57bb0ca85b049951be`.
- `https://github.com/crmne/spotifast`, observed head `d58fc2a5dca0b5532096c21fc4b5c6fc7fbec0ed`.
- `https://github.com/crmne/native-packages`, v0.7.0 commit `dc0cb1586894ca0239c53fd02d4da8064712671c`.

Inspect immutable revisions before copying. Preserve required notices. Do not assume disposable `/tmp` clones from the original research are still present.

Useful ZapFast references: `Cargo.toml`, `rust-toolchain.toml`, `AGENTS.md`, `CONTRIBUTING.md`, the actual source-level agent instructions if present, `src/main.rs`, worker/backend code, demo screenshot implementation, `native-packages.yaml`, release/packaging workflows, package templates, desktop assets, and installation tests.

Do not copy chat/audio services, credentials, auto-updaters, unrelated branding, platform matrices, or their complete dependency tree.

### Initial dependency choices

- Rust edition 2024; initially pin toolchain `1.98.0`, matching the researched reference. Confirm availability and compile the selected graph before committing. Record any necessary change.
- One Cargo package with library and binary targets; commit `Cargo.lock`.
- `eframe = "0.36"`, default features off, initial features `accesskit`, `glow`, `default_fonts`, `links`, `wayland`, `x11`, `persistence`.
- `egui = "0.36"`; use matching `egui_extras` only for needed image/SVG functionality.
- `rfd = "0.17"`, default features off, portal/Wayland configuration as validated by the reference. Keep manual path entry usable if a portal is unavailable.
- `directories = "6"`, `clap` derive, `serde`, `serde_json`, `log`, `env_logger`, `anyhow` or small typed errors where decisions depend on error kind.
- Blocking `reqwest` with rustls in background workers, archive libraries for ZIP and TAR/GZIP, hashing libraries, and a namespace-aware XML parser as required.
- Add process/locking primitives only where needed. Do not add an async runtime merely for convention.
- Explain non-obvious dependencies with short comments. Pin external forks by immutable commit.

Release profile: thin LTO, one codegen unit, stripped symbols, `panic = "abort"`. Optimize development dependencies where it materially helps GUI iteration. Do not rely on unwinding for correctness or operation recovery.

## 5. Architecture

Use one crate initially. Split responsibilities by behavior rather than implementing two complete competing toolchains.

| Component | Responsibility |
| --- | --- |
| App/UI | View state, navigation, user intents, validation, diffs |
| Operation coordinator | Jobs, cancellation, progress, mutation serialization, recovery state |
| SDK provider adapters | Package capabilities, listing, install, update, removal, licenses |
| Creation adapters | Supported device/image creation and device-profile discovery |
| AVD repository | Paths, identities, configs, external changes, safe mutation plans |
| Emulator runtime | Shared launch, process tracking, ADB mapping, boot readiness, stop, log capture |
| Doctor | Evidence-backed checks and actionable hints |
| Tool installer | Downloads, hashes, staging, safe extraction, runtime selection |

Android CLI is the preferred candidate for new SDK provisioning. Classic tools are a compatibility and advanced-creation provider. Confirm the default in an architecture decision after M0. A narrow validated classic adapter is preferable to an unsafe direct-file creation implementation.

Use the emulator executable and ADB directly for shared runtime behavior. Android CLI's start wrapper returning after boot does not necessarily give a handle to the long-lived emulator process.

Model capabilities explicitly, such as package listing, selected-image creation, device-profile discovery, and rename support. Return unsupported operations clearly. Do not substitute another image, API, or device name silently.

### Suggested layout

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
README.md
LICENSE
AGENTS.md
CONTRIBUTING.md
PRODUCT.md
PACKAGING.md
native-packages.yaml
src/main.rs
src/lib.rs
src/app.rs
src/ui/
src/operations/
src/providers/android_cli.rs
src/providers/cmdline_tools.rs
src/providers/mod.rs
src/sdk/packages.rs
src/sdk/repository.rs
src/avd/model.rs
src/avd/ini.rs
src/avd/repository.rs
src/runtime/emulator.rs
src/runtime/adb.rs
src/process.rs
src/download.rs
src/doctor.rs
src/settings.rs
src/paths.rs
src/demo.rs
tests/fixtures/
docs/AVDIUM-BOOTSTRAP.md
docs/STATUS.md
docs/VALIDATION.md
docs/decisions/
packaging/applications/dev.okazakee.Avdium.desktop
packaging/icons/dev.okazakee.Avdium.svg
packaging/arch/avdium-bin/PKGBUILD.in
packaging/test-install.sh
.github/workflows/ci.yml
.github/workflows/packaging.yml
.github/workflows/release.yml
```

This is a navigation guide, not a requirement to create empty modules. Add implementation files as their responsibilities become real.

### Background work

The UI emits actions; the app applies them after rendering. Workers send typed progress/results and request repaint. Never call a subprocess, make a network request, or perform an unbounded filesystem scan inside an egui frame.

Serialize mutations per SDK and per AVD where required. Keep status refresh and cancel delivery responsive while a mutation runs. The cancellation path must not wait behind the job it cancels.

## 6. SDK and path rules

### Discovery

Resolve explicit saved user choices, environment configuration, recognizable existing installations, and conventional defaults. Validate every candidate and show conflicts rather than choosing silently when they refer to different SDKs.

- `ANDROID_HOME` is the primary SDK environment variable.
- `ANDROID_SDK_ROOT` is deprecated; detect conflicts with `ANDROID_HOME`.
- Respect `ANDROID_USER_HOME`, `ANDROID_EMULATOR_HOME`, and `ANDROID_AVD_HOME` with documented/tool-confirmed precedence.
- Discover `cmdline-tools/<version>/bin`, including but not limited to `latest`.
- AVD discovery can work even if the referenced SDK/image is missing; display broken entries with useful reasons.
- Default fresh SDK location on Linux: `~/Android/Sdk`, configurable.
- Keep Avdium settings, caches, tool records, and logs in appropriate XDG locations. Do not put the user's SDK or AVDs into a private format.

Construct a resolved path context and pass consistent per-process environment overrides. Never edit shell startup files. Changing the configured SDK is selection, not permission to move it or rewrite every AVD.

An ambient `~/.androidrc` can alter Android CLI behavior. Test precedence and isolation instead of assuming `--sdk` neutralizes all ambient flags. Do not modify the user's file.

### Package catalog

Primary XML sources:

```text
https://dl.google.com/android/repository/repository2-3.xml
https://dl.google.com/android/repository/sys-img/google_apis/sys-img2-3.xml
https://dl.google.com/android/repository/sys-img/google_apis_playstore/sys-img2-3.xml
https://dl.google.com/android/repository/sys-img/android/sys-img2-3.xml
```

Parse namespaces, package path, revision, channel, licenses, host constraints, sizes, relative download URLs, and checksums. Retain unknown fields harmlessly where possible. Select by host/channel/revision, not document order. Disable external-entity/network expansion and bound input sizes.

Cache catalogs with freshness information. Offline catalog failure must not prevent listing or starting installed devices. Catalog UI does not justify implementing a custom SDK package installer: delegate installs to validated official tools.

Keep package IDs structured with provider-specific serialization. Classic tools use semicolons; Android CLI examples use slashes. Preserve identifiers such as `android-36.1` as strings. Do not parse API identifiers as integers or all revisions as strict semantic versions.

Before removal, show known AVD references and package dependencies. Block removal of an image used by a running AVD. Never uninstall unrelated packages automatically.

## 7. Provisioning, runtimes, licenses, and integrity

### Existing installation

Re-use validated tools without automatically upgrading them. An existing SDK is not consent to update its emulator, platform-tools, command-line tools, or images.

### Fresh installation

1. Resolve SDK and AVD locations and show the selected package plan.
2. Show relevant license texts before the action that requires acceptance; record the user's decision against the exact text/license identity.
3. Download the selected tools into private staging with progress and cancellation.
4. Check expected length where supplied and compare against an independently obtained expected checksum.
5. Extract safely, validate the staged installation, then promote it on the destination filesystem.
6. Probe the actual installed version and capabilities with metrics disabled where supported.
7. Install selected packages through the official provider and verify actual resulting state.
8. Offer creation with the user's chosen image/profile, explaining any additional compatibility-tool download needed.

Record tool URL, version, expected digest source, actual digest, install location, and installation result. Separate reviewed pins in a repository manifest from local installation state.

A digest calculated from a new download records identity; it does not authenticate that download by itself. Google's classic ZIP metadata currently provides SHA-1. Preserve that provenance; if adding a project SHA-256 pin, document how the initial archive was validated and reviewed.

### Java

Probe candidate runtimes per process. Select an already-compatible runtime if available. Otherwise offer a private Temurin runtime after resolving its exact artifact and published checksum.

Validate the pinned classic tools with that runtime. "Java >=17" is not the acceptance test. Do not reuse Android CLI's embedded runtime unless it exposes a supported executable and compatibility is demonstrated.

### License consent

Do not equate a marker named `tos_displayed` with consent. Do not pipe an unbounded `yes` into tools after showing a generic terms URL. Handle each required license or an explicitly enumerated group of exact texts, then send only the answers covered by that decision.

Respect consent already validly recorded by installed tooling without repeatedly asking for the same terms. New license text requires a new decision. If a provider accepts terms implicitly or cannot expose the required consent boundary, mark that integration blocked and use a supported path.

### Update behavior

The verified launcher pin covers only the launcher. Before promising reproducibility, investigate downloaded payload identity, shared CLI directories, update checks, a newer existing installation, and offline startup. Record actual behavior.

Avdium never silently runs `android update`. Do not silently replace user-managed tools. Surface updates as deliberate operations, and avoid upgrading an emulator while its devices are running.

### Filesystem installation safety

- Unique temporary locations; no collisions between concurrent app instances.
- Staging and final rename on the same filesystem.
- Reject traversal, absolute archive paths, and symlink/hardlink escapes; preserve required executable permissions.
- Bound download/extraction sizes and account for temporary space.
- Roll back or expose a recoverable incomplete install; do not claim every external install is transactional.
- Do not overwrite `cmdline-tools/latest` blindly. Prefer a versioned installation location where the official tool layout supports it.
- Resolve archive top-level layout rather than assuming every future ZIP is identical.

## 8. AVD model and safe operations

### Identity and discovery

Read companion `.ini` files from the resolved AVD home, resolve their data directories, and parse `config.ini`. Preserve path provenance and expose missing images/configs as broken devices.

Do not confuse AVD name, display name, data-directory identity, process ID, ADB serial, and device profile. Maintain explicit mappings. Verify identity before sending a destructive or stop operation to a serial that may have been reused.

### Configuration

Use a minimally changing INI representation that preserves unknown keys, comments, order, and line endings where feasible. Reject malformed/conflicting data when it prevents a safe edit rather than normalizing it away.

Initial editable settings: RAM, CPU cores, keyboard, graphics mode, cameras, and selected boot behavior. Additional validated settings can include resolution/density and network simulation. Use supported ranges and clear advanced controls.

Keep `target`, image path, ABI, CPU architecture, and tag consistent. A `PlayStore.enabled` flag does not turn a non-Play image into a Play Store image. Treat image capabilities as authoritative.

The supplied Pixel_9 config is a user-modified sample, not a canonical device profile. Device skins are optional and must be discovered, not assumed to ship for every model. Preserve valid profile identity fields; do not invent hash values.

### Mutation rules

1. Check identity, active processes, relevant emulator locks, and external file changes.
2. Prepare an operation plan and a human-readable diff/summary.
3. For overwrite/delete/wipe, obtain the appropriate user action/confirmation in the product UI.
4. Recheck conflicts immediately before committing the mutation.
5. Use same-directory temporary files, fsync as appropriate, and atomic replacement. Preserve permissions.
6. For multi-file operations, keep a recoverable journal or equivalent state. Two atomic renames are not one atomic transaction.
7. Reload actual state and report the result or partial failure accurately.

An app-owned lock coordinates Avdium instances, not Android Studio. Never claim that it prevents external changes. If the external tool locking contract is uncertain, refuse unsafe edits while activity is uncertain and document the limitation.

### Creation

Use the validated provider's supported creation API. Classic `avdmanager` may be required for selected image/profile creation. Capture supported options from the installed build and handle hardware prompts without hanging.

Direct generation of `.ini` and `config.ini` is experimental until a real matrix demonstrates correct boot and interoperability. It is not required for v0.1 if official-tool creation works. Do not silently route normal users through an untested template.

### Delete, wipe, rename, and duplicate

- Never recursively delete an arbitrary directory solely because an imported `.ini` points at it. Validate structure, canonical target, symlink behavior, and exact device identity. Show an external target clearly; reject dangerous or ambiguous destinations.
- Refuse delete, wipe, edit, and move while an AVD is running or its state cannot be established safely.
- Wipe via the supported emulator operation. Do not simulate it by removing only `userdata-qemu.img`. Document that SD-card data is separate.
- Rename/move remains deferred until a provider and recovery behavior are validated.
- An optional early duplicate action may create a fresh AVD with copied settings and the same image. Label it explicitly as a fresh device. Full user-data/snapshot cloning is deferred.
- No existing data-partition shrink or arbitrary resize promise in v0.1. A config edit alone is not evidence of safe disk/filesystem resizing.

## 9. Runtime and subprocess contract

### Core commands

These illustrate intended operations. Resolve tool paths and build argument arrays in code.

```sh
"$SDK/emulator/emulator" -list-avds
"$SDK/emulator/emulator" -accel-check
"$SDK/emulator/emulator" -avd NAME
"$SDK/emulator/emulator" -avd NAME -no-snapshot-load
"$SDK/emulator/emulator" -avd NAME -no-snapshot
"$SDK/platform-tools/adb" devices -l
"$SDK/platform-tools/adb" -s SERIAL shell getprop sys.boot_completed
"$SDK/platform-tools/adb" -s SERIAL emu kill
```

Use `-no-snapshot-load` for cold boot. It differs from disabling both snapshot load and save. Do not use the unverified raw-emulator `-cold-boot` flag.

Default graphics to `auto`. The current documented general fallback is `software`, but verify support in the installed build. `swiftshader_indirect` is deprecated from emulator 36.4.9 and must not be the universal fallback.

Discover supported advanced options rather than passing a generic bag of flags to every emulator version. Headless operation and logcat can follow as validated runtime capabilities.

### State and identity

Distinguish stopped, starting, ADB-connected, boot-complete, offline/unresponsive, stopping, failed, and unknown. `adb devices` showing `device` is not a boot-complete signal. Use bounded readiness polling and preserve useful failure logs.

Map ADB serials to AVD identity using a verified emulator/ADB query, such as `adb -s SERIAL emu avd name` if supported in the selected version. Capture fixtures. Discover externally launched devices as well as Avdium-owned ones.

Use the configured ADB server endpoint, normally TCP 5037. Do not invent a `~/.android/adb.5037` discovery contract. Avoid `adb kill-server` as routine recovery because the server may serve other development tools and physical devices.

### Process mechanics

- Separate argv and environment from display formatting; shell escaping is for a copyable command preview only.
- Concurrently drain stdout/stderr; handle prompts without newline, carriage-return progress, long lines, invalid UTF-8, EOF, and output limits.
- Bound retained logs and UI events, with sensible truncation/rotation.
- Support cancellable downloads and child processes without blocking the coordinator.
- Signal owned process groups when cancellation requires it, wait gracefully, then force termination if needed. Reap processes.
- Do not treat wrapper exit or a successful spawn as emulator readiness.
- Persist enough launch metadata to rediscover a device after app restart without trusting stale PIDs.
- Default GUI close leaves already-running emulators running. Log destinations must survive the GUI exit without filling an unread pipe. An explicit stop remains available.
- Cancellation of provisioning must report partial changes and reconcile actual SDK state before retrying.

## 10. User experience

Create a restrained desktop utility with clear typography, spacing, keyboard focus, accessible labels, and light/dark support. Establish a small token set for spacing, colors, text sizes, and status. Avoid copying the visual density of Android Studio.

### Main surfaces

| Surface | Contents |
| --- | --- |
| Devices | Name, image/API/tag/ABI, status, essential hardware, start/cold-boot/stop, edit, wipe, delete, folder, command preview |
| Setup | SDK path, package plan, image selection, size estimates, terms, progress, retry/cancel |
| Create/edit | Validated profile/image selectors, settings, advanced fields, save diff |
| SDK | Installed packages, available updates/images, filters, selected operations and dependency impact |
| Doctor | Pass/warn/fail/unknown checks with evidence and appropriate fixes |
| Activity | Jobs, command previews, progress, logs, cancellation and partial-failure detail |
| Settings | SDK/AVD paths, theme, logging, advanced provider/runtime preferences |
| About/diagnostics | Avdium and tool versions, license notices, redacted export |

For M1, implement Devices, Doctor, Settings, and Activity sufficiently to manage an existing SDK. Do not ship clickable placeholder buttons pretending later actions work. Disabled capabilities need a brief reason.

No-SDK startup should explain what is needed and offer setup when implemented. Offline operation should retain installed-device functionality. Backend choice belongs in advanced settings or recovery, not as the first onboarding question.

For package downloads, show download size and estimated disk needs separately. Do not describe the whole SDK as a 5 MB installation. Sparse AVD images require separate logical and allocated size reporting if showing disk usage.

Raw command output may contain local paths. Keep it local, redact exported diagnostics by default, and do not log license text, device secrets, or tokens indiscriminately. Let the user inspect an export before sharing it.

## 11. Doctor

Every result contains a status, observed evidence, and next action where useful. Unknown means unknown, not fail or pass.

- SDK existence, actual layout, selected paths, permissions, conflicting environment, and missing packages.
- Tool executable/version/capabilities; Java only when the selected provider needs it.
- `/dev/kvm` access and emulator `-accel-check`; distinguish permissions, firmware virtualization, modules, and unsupported virtualization environments.
- Graphics devices and necessary GL/EGL libraries. Only suggest a graphics fallback supported by the selected emulator.
- Required GUI/portal libraries separately from emulator runtime libraries.
- Free space at actual SDK, staging, and AVD locations; memory information and resource recommendations.
- Network reachability only when needed for setup or an explicit check; timeouts must not freeze startup.
- Broken AVD paths/images, currently active devices, package-use conflicts, and ADB endpoint issues.

Do not run root changes. Provide distro-specific commands only when the diagnosed cause supports them. Group membership is one possible KVM fix, not the diagnosis itself.

## 12. M0: focused validation matrix

Use a disposable test user/container or another proven isolation method for tools that write user-global state. A temporary SDK directory alone does not isolate `.android`, `.androidrc`, AVD locations, or the ADB server. Do not mutate the real user's home to create the test environment.

| Check | Required evidence / decision |
| --- | --- |
| Fresh CLI bootstrap | Actual downloads, runtime requirement, writes, versions, prompts, and decline behavior |
| Repeat/offline invocation | Whether it reuses payloads, makes network checks, and remains usable offline |
| Existing newer CLI | Launcher vs application version, shared paths, update/isolation behavior |
| Metrics | Verify every Avdium CLI invocation includes the supported opt-out; separate emulator/guest behavior |
| `.androidrc` conflict | Effective SDK/flags with ambient configuration and Avdium overrides |
| SDK listing/install | Exact IDs, versions, channel behavior, output formats, progress, license boundaries |
| Creation | Exact supported positional/flag syntax and chosen-image capabilities |
| Stop/identity | Whether wrappers expect serial/name; direct runtime mapping verified |
| Classic tools + Java | Actual pinned-build compatibility and prompt handling, not a generic Java minimum |
| Package discovery | Host/channel selection and offline installed-state reconciliation |
| Test isolation | Before/after evidence that unrelated SDKs/AVDs are untouched |

There is a documented/reported syntax discrepancy: current docs describe `android emulator create --profile=...` and stopping by serial; the original report described a positional profile and stopping by name. Resolve it with complete help and successful runs of the exact installed binary. Do not encode either assumption without that evidence.

A help command or list command can itself bootstrap or write caches. Classify commands by observed side effects. If new license acceptance is required and unavailable, preserve that blocker and continue independent implementation work.

Produce `docs/VALIDATION.md` with environment, results, exact commands, exit codes, versions, unresolved questions and their consequence. Use small decision records for default provider, managed Java, and runtime lifecycle choices.

## 13. Test strategy

### Offline automated tests

- INI preservation and minimal diffs, including unknown keys and malformed input.
- SDK/AVD resolution and conflicting environment settings.
- Package ID serialization, repository namespace/channel/host/revision handling.
- Tool output parsing against complete versioned fixtures and deliberately malformed output.
- Process prompts without newline, mixed streams, cancellation, timeout, large output, and child-process cleanup.
- Conflict detection, unsafe paths/symlinks, and interrupted multi-file operations.
- Package removal dependency checks and runtime identity/readiness state transitions.
- Settings persistence and recovery from interrupted writes.

Fake executables for `android`, `sdkmanager`, `avdmanager`, `emulator`, `adb`, and `java` should record arguments/environment and simulate success, failures, hangs, prompts, and drift. Tests must not accidentally fall through to real executables.

All ordinary tests use temporary paths and no network. Network and real-emulator tests are explicit opt-in commands or ignored integration tests.

### GUI verification

The `demo` feature uses deterministic synthetic devices, SDK state, operations, and failures. It must never discover or mutate a real SDK.

Implement layout tests for actual implemented screens and a reproducible screenshot mode. Inspect the reference renderer before promising it needs no display server. A documented virtual-display renderer is acceptable initially if a truly headless renderer is not yet available; state which is implemented.

Check no-SDK, existing devices, broken device, busy operation, failure, long names/paths, and light/dark layouts. Include keyboard/accessibility behavior in manual acceptance. Tests should verify behavior, not mirror the implementation mechanically.

### Real integration tests

Use disposable AVDs with unique names and dedicated data directories. Verify actual targets before boot/edit/wipe/delete. Existing user devices are discovery fixtures only unless separately authorized.

Run start, readiness, stop, edit-on-stopped-device, cold boot, wipe, and removal on disposable devices. Test two devices and one externally launched device. Record actual host, emulator, image and provider versions.

Package installation containers do not need KVM for binary/desktop/dependency checks. Real emulator CI requires a runner with working virtualization and exposed `/dev/kvm`; containers do not inherently prohibit it.

### Repository checks

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps
```

Generate the initial lockfile deliberately before using `--locked`. Do not weaken lints or remove meaningful assertions to obtain green checks. Distinguish compiled, automated-tested, visually inspected, and manually exercised outcomes.

## 14. Packaging and release

Use native-packages YAML as the single source of package metadata. Pin native-packages 0.7.0 and nFPM 2.47.0 initially; verify current schema and command syntax from the pinned source.

Build Linux x86_64 only. Adapt ZapFast's actual workflow chain: release invokes its repository packaging workflow, which in turn integrates native-packages. Do not paste a guessed reusable workflow interface.

### Artifacts

```text
avdium-vVERSION-x86_64-unknown-linux-gnu.tar.gz
avdium-vVERSION-x86_64-unknown-linux-gnu/
  avdium
  README.md
  LICENSE
  packaging/applications/dev.okazakee.Avdium.desktop
  packaging/icons/dev.okazakee.Avdium.svg
```

The actual release version replaces `VERSION` during packaging. Follow the pinned tool's placeholder syntax in templates, not the literal example above.

Native package contents:

- Binary at `/usr/bin/avdium`.
- Desktop entry under `/usr/share/applications/`.
- Icon under `/usr/share/icons/hicolor/scalable/apps/`.
- License/readme in conventional package-specific locations.
- No redistributed Google tools or SDK/image payloads.

Set desktop `Exec=avdium`, correct application identity, and appropriate development categories. Match the Wayland app ID and X11 window association to the desktop entry using supported framework configuration, then test it.

Derive package dependencies from Avdium's build/runtime requirements. Do not copy ZapFast audio dependencies or assume a Rust binary has no runtime dependencies. Do not add QEMU as a dependency: the Android emulator supplies its own engine. Java is not a prerequisite for opening Avdium.

Choose and document the minimum glibc/distro baseline before the first release. A build on Ubuntu 24.04 does not automatically support every older Linux distribution. Probe dynamically loaded GUI dependencies where ELF linkage misses them.

### CI/release behavior

- Regular CI runs offline repository checks; separate GUI/dependency checks where necessary.
- Package jobs install into clean supported Ubuntu/Debian/Fedora containers, run `avdium --version`, check runtime libraries/assets, then uninstall while preserving user settings.
- AUR recipe uses release checksums and generates valid `.SRCINFO`. Stage it before any publication.
- Pin third-party Actions by reviewed full commit SHA; minimize permissions and avoid leaking signing/AUR secrets to untrusted jobs.
- Release tags must point at approved commits reachable from the intended mainline. Validate version consistency.
- Produce checksums and artifact attestations using the pinned native-packages action where supported.
- Write release notes describing implemented behavior and validation, not aspirational features.
- Application self-updating and update-signing infrastructure are deferred.

At bootstrap, validate the packaging skeleton as far as available artifacts permit. Do not present an unexecuted release pipeline as verified. External publication requires an established destination and authorization through the actual workflow.

## 15. Milestones and completion criteria

### Foundation

Create the Cargo project, app identity, lockfile, basic window, original icon, CI, and concise repository instructions. `PRODUCT.md` holds product scope; `CONTRIBUTING.md` holds checks and contribution guidance; `AGENTS.md` holds operational constraints without overriding installed harness rules.

Completion: clean build, actual app window or accurately reported display blocker, `--version` without initializing the GUI/network, baseline checks, and no-SDK state.

### M0: evidence and provider decisions

Perform section 12 checks. Capture complete fixtures and write the short provider/runtime decisions. Treat missing real-host access as a declared validation gap, not a reason to invent output or halt all coding.

Completion: supported operations and unresolved risks are documented, tool pins have correct provenance, and the next implementation uses demonstrated capabilities.

### M1: existing-SDK manager

Implement paths/settings, discovery, device model, doctor, runtime start/cold-boot/stop/readiness, logs, operation cancellation, and demo screens.

Completion: list real devices without modifying them; exercise runtime actions only on disposable devices; correct status/identity handling; usable failure/offline paths; automated and visual checks recorded.

### M2: provisioning and creation

Implement downloads, consent handling, catalog/image picker, provider package operations, private runtime if required, and supported selected-image creation. Add classic creation support if needed without duplicating runtime lifecycle logic.

Completion: a fresh isolated setup without system Java can provision tools and a chosen image, create a disposable AVD, boot it, and stop it; cancellation/failure/retry and consent refusal behave correctly. If the host cannot run this test, the milestone remains implemented but unvalidated.

### M3: safe mutation

Implement config diffs, conflict checks, supported edits, wipe, dependency-aware package removal, and validated delete. Fresh-settings duplicate is optional. Keep full data cloning, move/rename, snapshot management, and resizing deferred.

Completion: boot after settings changes; no mutation while running; refusal of unsafe paths and stale edits; disposable wipe/delete tested; interrupted operations have a documented recovery route.

### M4: release readiness

Finish native packages, install/remove tests, runtime dependency checks, supported-host documentation, privacy notices, release notes, and staged recipes.

Completion: build/install artifacts validated on claimed environments, all required checks green, remaining limitations explicit, publication materials ready. A published release is a distinct externally authorized action.

## 16. Guardrails for product claims

- Say "no system Java installation required," not "zero Java."
- Report total downloads and disk needs, not only the launcher size.
- Standard file compatibility is a design goal with tested cases, not proof that every operation interoperates.
- Both classic SDK tools are deprecated; compatibility support may still be practical.
- No telemetry in Avdium does not mean an Android guest makes no network requests or collects no data.
- Runtime downloads can follow redirects to GitHub/CDNs; do not claim an exhaustive two-domain network list.
- Use documented license boundaries and preserve upstream notices; do not make blanket redistribution/legal claims.
- Linux ARM and Windows/macOS are future platforms. The reviewed CLI docs currently list Windows emulator commands as disabled.
- Flatpak is deferred for scope and integration work. Do not repeat an unverified blanket claim about Flathub's AI policy.
- Avoid em dashes in user-facing text. Prefer direct, useful error messages over marketing language.

## 17. Handoff and ongoing status

Keep `docs/STATUS.md` short and current:

- Implemented behavior.
- Current milestone and next concrete task.
- Actual checks run with results.
- Evidence not yet collected and the capability it blocks.
- Repository state and local commits, following actual harness policy.

At the end of an execution session, report what works, how it was verified, and what remains. Do not replace implementation with a restatement of this brief. Do not claim screenshots, real-device boots, package installs, or release publication that did not happen.

## 18. Primary sources

Use these for implementation questions. Re-check a source when its relevant behavior changes; do not repeatedly redo the whole research phase.

- [Android CLI](https://developer.android.com/tools/agents/android-cli)
- [sdkmanager](https://developer.android.com/tools/sdkmanager)
- [avdmanager](https://developer.android.com/tools/avdmanager)
- [Emulator command line](https://developer.android.com/studio/run/emulator-commandline)
- [Emulator acceleration and GPU modes](https://developer.android.com/studio/run/emulator-acceleration)
- [Android environment variables](https://developer.android.com/tools/variables)
- [ADB](https://developer.android.com/tools/adb)
- [SDK terms](https://developer.android.com/studio/terms)
- [Official package repository](https://dl.google.com/android/repository/repository2-3.xml)
- [Google emulator container scripts](https://github.com/google/android-emulator-container-scripts)
- [ZapFast](https://github.com/crmne/zapfast)
- [Spotifast](https://github.com/crmne/spotifast)
- [native-packages](https://github.com/crmne/native-packages)
- [Existing AVD Manager UI](https://github.com/statickidz/avd-manager-ui)
- [Existing AVD Launcher](https://github.com/gustavomdsantos/avd-launcher)

## Appendix: supplied research captures

The captures below are copied from the original research, with personal home paths replaced by `/home/example`. They have **not** been rerun by this document's author. Preserve that provenance when using them as seed fixtures. Abridged output is suitable for targeted parser cases, not proof of the full grammar. Replace or supplement it with complete versioned captures during M0.

### A. Supplied command output


`android info --no-metrics`:

```
sdk: /home/example/Android/Sdk
version: 1.0.16406183
launcher_version: 1.0.16406183
```

`android emulator list --no-metrics`:

```
Pixel_9
Pixel_Tablet
Pixel_9_Pro
```

`android emulator create --list-profiles --no-metrics`:

```
large_desktop
medium_desktop
medium_phone
medium_tablet
small_desktop
small_phone
```

`android sdk list --no-metrics` (abridged, installed section):

```
Installed packages:
  build-tools/36.1.0                                                    36.1.0                             Android SDK Build-Tools 36.1
  emulator                                                              36.2.12         ->        37.1.11  Android Emulator
  platform-tools                                                        36.0.0          ->        37.0.1   Android SDK Platform-Tools
  platforms/android-36                                                  2.0.0                              Android SDK Platform 36
  system-images/android-36.1/google_apis/x86_64                         4.0.0                              Google APIs Intel x86_64 Atom System Image
  system-images/android-36.1/google_apis_playstore/x86_64               3.0.0           ->        4.0.0    Google Play Intel x86_64 Atom System Image
  system-images/android-37.0/google_apis_playstore_ps16k/x86_64         4.0.0           ->        7.0.0    16 KB Page Size Google Play Intel x86_64 Atom System Image
```

`emulator -list-avds`:

```
Pixel_9
Pixel_9_Pro
Pixel_Tablet
```

`emulator -accel-check`:

```
accel:
0
KVM (version 12) is installed and usable.
accel
```

`emulator -version`:

```
Android emulator version 36.2.12.0 (build_id 14214601) (CL:N/A)
```

`android emulator start --help` (abridged):

```
Usage: android emulator start [-h] [--cold] [--headless] <device>
Launches the specified virtual device. This command will return when the
emulator is fully started and ready to use
```

`android sdk install --help` (abridged):

```
Usage: android sdk install [-h] [--beta] [--canary] [--force]
                           <package>[@<version>]
```

`android emulator --help` subcommands: create, list, remove, start, stop.
`android sdk --help` subcommands: install, list, remove, update.
`android --help` also has: update, info, init, install, run, create, describe, layout, screen, docs, skills, studio, completion.

### B. Supplied Pixel_9 configuration

This fixture contains user-selected settings. Do not use its RAM, CPU, or display values as factory profile defaults.


```
AvdId=Pixel_9
PlayStore.enabled=false
abi.type=x86_64
avd.ini.displayname=Pixel 9
avd.ini.encoding=UTF-8
disk.dataPartition.size=16G
fastboot.chosenSnapshotFile=
fastboot.forceChosenSnapshotBoot=no
fastboot.forceColdBoot=yes
fastboot.forceFastBoot=no
hw.accelerometer=yes
hw.arc=false
hw.audioInput=yes
hw.battery=yes
hw.camera.back=virtualscene
hw.camera.front=emulated
hw.cpu.arch=x86_64
hw.cpu.ncore=6
hw.dPad=no
hw.device.hash2=MD5:5478e3411cc0e0441240e736eb14c07a
hw.device.manufacturer=Google
hw.device.name=pixel_9
hw.gps=yes
hw.gpu.enabled=yes
hw.gpu.mode=auto
hw.gyroscope=yes
hw.initialOrientation=portrait
hw.keyboard=yes
hw.lcd.density=420
hw.lcd.height=2424
hw.lcd.width=1080
hw.mainKeys=no
hw.ramSize=8192
hw.sdCard=yes
hw.sensors.light=yes
hw.sensors.magnetic_field=yes
hw.sensors.orientation=yes
hw.sensors.pressure=yes
hw.sensors.proximity=yes
hw.trackBall=no
image.sysdir.1=system-images/android-36.1/google_apis/x86_64/
runtime.network.latency=none
runtime.network.speed=full
showDeviceFrame=yes
skin.dynamic=yes
skin.name=pixel_9
skin.path=/home/example/Android/Sdk/skins/pixel_9
tag.display=Google APIs
tag.displaynames=Google APIs
tag.id=google_apis
tag.ids=google_apis
target=android-36.1
vm.heapSize=228
```

Companion `<Name>.ini`:

```
avd.ini.encoding=UTF-8
path=/home/example/.android/avd/Pixel_9.avd
path.rel=avd/Pixel_9.avd
target=android-36.1
```
