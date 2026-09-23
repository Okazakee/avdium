# Status

Date: 2026-09-23
Version: `0.1.0-dev`
Current milestone: M1 (existing-SDK manager), implementation complete, with the
real-device runtime checks not yet executed

## Implemented behavior

**Foundation.** One Cargo package with library and binary targets, edition
2024, toolchain pinned to `1.98.0` in `rust-toolchain.toml`, `Cargo.lock`
committed. Release profile uses thin LTO, one codegen unit, stripped symbols and
`panic = "abort"`.

**Paths and settings.** SDK root resolution with documented precedence
(settings, `ANDROID_HOME`, deprecated `ANDROID_SDK_ROOT`, `~/Android/Sdk`), AVD
home resolution (settings, `ANDROID_AVD_HOME`, `ANDROID_EMULATOR_HOME/avd`,
`ANDROID_USER_HOME/avd`, `~/.android/avd`), `cmdline-tools/<version>/bin`
discovery, and conflict reporting that never resolves a disagreement silently.
Settings persist as JSON under the XDG config directory with atomic writes and
recovery from a corrupt file, preserving unknown keys.

**Device model.** Companion `.ini` and `config.ini` parsing that preserves
comments, unknown keys, duplicate keys, unparsable lines, order and line
endings. `path.rel` is resolved against the user home, matching the real
companion file in the brief. Broken devices are listed with a reason rather than
dropped. Discovery works with no SDK installed.

**Process infrastructure.** Argument arrays only, never a shell. Concurrent
stdout/stderr draining, partial-line reporting for prompts without a newline,
carriage-return progress, invalid UTF-8 tolerance, bounded retained output with
reported truncation, cancellation that signals the whole process group, and
guaranteed reaping.

**Providers.** Explicit capability matrices for Android CLI and the classic
tools. Every invocation passes the supported `--no-metrics` opt-out. Android CLI
is correctly reported as **unable** to create a device from a chosen image.

**Package catalog.** Namespace-aware parser for the official repository XML with
`<!DOCTYPE` refused outright, only predefined and numeric entities resolved, and
input size bounded. Selection is by host, channel and revision, not document
order.

**Runtime.** Emulator and ADB driven directly. Cold boot uses
`-no-snapshot-load`; `-cold-boot` is never used. Graphics default to `auto` and
`swiftshader_indirect` is never selected. Wipe uses the emulator's `-wipe-data`.
Readiness requires `sys.boot_completed`, not merely an ADB `device` state.
Launch records are verified against `/proc/<pid>/cmdline` so a reused pid is
never trusted, and dead children are reaped.

**Doctor.** Seventeen checks, each with a status, the evidence it was based on
and a next action. `Unknown` is distinct from pass and fail, including for a
check that was never run.

**Operations.** Background jobs with cancellation, progress, bounded per-job
logs and per-resource mutation serialization. Cancellation flips a flag under a
lock the worker does not hold, so it cannot wait behind the job it cancels.

**Provisioning.** A reviewed pin table in source, a pure planner that
computes the smallest set of actions to reach a goal, license decisions bound
to the exact text that was displayed, and an installer that stages inside the
destination filesystem, verifies size and digest, resolves the archive layout
rather than assuming it, validates the result, refuses to overwrite an existing
installation, and records what it did. An existing installation is never
scheduled for upgrade. The Java already on the machine is reused; a private
runtime is only proposed, and stays blocked until its exact release and
checksum are pinned.

**Downloads.** Streaming download with progress, cancellation, size and digest
verification, and explicit provenance for expected digests. Safe ZIP and
TAR/GZIP extraction rejecting absolute paths, `..`, symlinks and hardlinks, with
bounded uncompressed size and preserved executable bits.

**UI.** Five screens: Devices, Doctor, Activity, Settings, About. A no-SDK state
that explains what is needed, an offline-tolerant device list, inspectable
command previews, and disabled controls that always carry a reason.

**Demo mode.** Deterministic synthetic devices, packages and doctor results. The
module performs no I/O at all, so it cannot reach a real SDK; a test asserts
that no demo path escapes `/demo`.

## Checks run

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --locked --all-targets -- -D warnings` | clean |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean |
| `cargo test --locked --all-targets` | 264 passed, 0 failed, 1 ignored |
| `cargo test --locked --all-targets --all-features` | 265 passed, 0 failed, 1 ignored |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps` | clean |
| `avdium --version`, `paths`, `tools`, `devices`, `doctor --probe`, `capabilities` | exercised against the real host, read-only |
| GUI launch | window opened on the live Wayland session; 5 screenshots captured |

Evidence levels: the repository checks are automated-tested. The real-host
reporting commands are manually-exercised. The screenshots are
visually-inspected. Real device start, boot, stop, wipe and delete are **not**
exercised.

## Evidence not collected, and what it blocks

- **Classic tools are not installed on this host.** `providers/cmdline_tools.rs`
  is compiled and unit-tested but has never been run against a real build, so
  M2 selected-image creation is implemented and unvalidated. The module says so
  in its own documentation.
- **No Java runtime is pinned.** No Temurin version or checksum is recorded
  anywhere, because the Adoptium asset resolution was not performed.
- **No disposable AVD was created**, so real start, readiness, cold boot, stop,
  wipe and delete remain unvalidated. The existing user devices were used for
  read-only discovery only.
- **`.androidrc` precedence** is unverified, because no such file exists here.
- **Fresh CLI bootstrap and offline invocation** were not performed.
- **The packaging pipeline was not executed.** No package was built, installed
  or published. `native-packages.yaml` is based on the pinned 0.7.0 schema but
  its dependency lists are explicitly marked as needing validation.
- **Which windowing backend winit selected** was not confirmed, so X11-only
  operation is unverified.

Full detail, with commands and exit codes, is in `docs/VALIDATION.md`.

## Capability matrix

| Capability | Android CLI | Classic tools |
| --- | --- | --- |
| List packages | yes | yes |
| Install, update, remove packages | yes | yes |
| List devices | yes | yes |
| Create from a profile | yes | yes |
| Create from a chosen image | **no** | yes |
| List device profiles | yes | yes |
| Remove a device | yes | yes |
| Rename a device | no | no |
| Start, stop a device | yes (wrapper) | no |
| License consent boundary | implicit, blocked | per-package text |

Runtime start and stop are performed by the emulator and ADB directly rather
than through either provider, because a wrapper that returns once a device is
ready is not a handle to the emulator process.

## Next concrete task

Wire the provisioning layer into a Setup surface, then use it to install the
classic tools and create a disposable AVD with a unique name and a dedicated
data directory. Exercise start, readiness, cold boot, stop and wipe against it,
recording the emulator and image versions. That is the remaining M1 completion
evidence and the prerequisite for M3 mutation work. `docs/ROADMAP.md` owns the
order after that.

## Repository state

Linear history on `main`. Nothing is pushed, tagged or published. `publish =
false` in `Cargo.toml` and the release workflow requires an explicit
authorization input, so publication cannot happen by accident.
