# Security policy

## Reporting a vulnerability

Use GitHub's private vulnerability reporting on this repository
("Security" then "Report a vulnerability"). Please do not open a public issue
for a security problem.

This is a single-maintainer project without a security team or a response-time
commitment. Reports are read and answered as time allows, and a fix is credited
unless you ask otherwise.

## Supported versions

**No version of Avdium is currently supported.** The project is at
`0.1.0-dev`, has never been released, and publishes nothing. There are no
release tags, no packages and no supported release branches. Fixes land on
`main` only.

## What Avdium is, and what that means for scope

Avdium is a local desktop application that manages an Android SDK and Android
virtual devices on the machine it runs on. It has no server component, no hosted
service, no accounts, no telemetry and no update mechanism. It does not collect
or transmit anything about the user.

Because of that, several classes of report are out of scope here:

- **The Android guest's own behavior.** An emulator booted by Avdium runs
  Android, which makes its own network requests and may collect data. Avdium
  neither controls nor claims to control that.
- **Third-party tool behavior.** Avdium invokes Google's Android CLI, the
  classic `sdkmanager`/`avdmanager`, the emulator, `adb`, and the Java runtime
  already installed on the machine. A vulnerability in those is theirs, not
  Avdium's. Pass supported telemetry opt-outs, which Avdium does, is not a
  security control.
- **Anything requiring a hosted service**, because none exists.
- **Reports about downloads being slow, large, or fetched from Google's CDN.**

## Areas where a report would be most valuable

These are the places where Avdium processes input it does not control, or
crosses a privilege or trust boundary. Reports here are the most useful.

- **Argument construction and execution.** Avdium executes argument arrays and
  never a reconstructed shell string. A case where a value from settings, a
  device name, a package identifier or a path is interpreted by a shell, or
  where `CommandSpec::preview` is ever executed rather than displayed, is a real
  finding.
- **Archive extraction.** Provisioning downloads ZIP and TAR/GZIP archives and
  extracts them into a staging directory. Absolute paths, `..` components,
  symlinks and hardlinks are rejected, and uncompressed size is bounded. A path
  that escapes the destination, a link entry that is materialised rather than
  rejected, or a way to exceed the size bound is a real finding.
- **Download integrity.** Downloads are verified against a size and a digest
  before promotion, and the provenance of an expected digest is recorded.
  A path where verification is skipped, or where a weak digest is treated as
  strong, is a real finding.
- **`unsafe` code.** There are a small number of `unsafe` blocks, all for
  process-group signalling and termination. A soundness problem there is a real
  finding.
- **The no-sudo boundary.** Avdium must never require or request elevated
  privileges, and must never modify shell startup files or global Git
  configuration. A code path that does either is a real finding.
- **State isolation.** Discovery and mutation are expected to touch only the
  configured SDK, AVD home and Avdium's own XDG directories. A path that writes
  outside those is a real finding.

## What is not claimed

Avdium does not claim, and has not verified, that every operation interoperates
with every official tool, that any download is trustworthy beyond its published
digest, or that the application is free of memory-safety or logic defects. It
has not been audited. Treat `0.1.0-dev` accordingly.
