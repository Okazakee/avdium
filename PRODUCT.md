# Avdium product contract

This file owns durable product scope. `docs/AVDIUM-BOOTSTRAP.md` is the
original bootstrap brief and records the reasoning and evidence baseline at
project start. Implementation details live in source and tests, not here.

## Problem

Developers using React Native, Expo, Flutter or similar workflows often need
Android emulators without the rest of Android Studio. The official command-line
tooling exists, but installation, image selection, device configuration,
diagnostics and lifecycle management require unnecessary manual work.

## Product

A small native desktop application that discovers or provisions an SDK, manages
the emulator packages and images needed by its users, creates and configures
AVDs, and starts and stops them with useful diagnostics.

The value is the complete setup and management experience. A launcher-only
interface is an intermediate milestone, not the product.

## Core principles

- No Android Studio, Gradle project, IDE or account required to use Avdium.
- No system Java installation required. Providers may use a private runtime or
  an already-compatible Java installation.
- Standard SDK and AVD files remain interoperable with official tools.
- User-owned installation locations; no sudo during normal operation.
- Existing installations are discovered and respected.
- User-selected downloads with integrity checks and explicit handling of
  required terms.
- Commands and operation output are inspectable. Argument arrays are executed,
  never a reconstructed shell command.
- Actionable diagnostics for SDK paths, KVM, graphics, runtime dependencies,
  disk and tool failures.
- Avdium has no telemetry or hosted service. Supported tool telemetry is
  disabled for Avdium-invoked operations, and third-party behavior is described
  accurately rather than denied.
- All blocking work stays off the UI thread.

## v0.1 release scope

- Linux x86_64.
- X11 and Wayland, with actual validation status documented separately.
- Existing SDK discovery and configurable SDK/AVD locations.
- Setup wizard for required packages and a selected system image.
- Package listing; selected install/update/remove operations with dependency
  checks.
- AVD listing, supported device/image creation, start, cold boot, stop, safe
  settings edits, wipe and delete.
- Device status, bounded log viewer, doctor and redacted diagnostic export.
- Demo mode and repeatable UI screenshots.
- Release tarball, DEB, RPM and an AUR binary recipe using native-packages.

Creation with a user-selected image is a release requirement. If the Android
CLI cannot implement it, a validated classic-tools creation adapter is used. A
generic profile is never silently substituted for a chosen API or image.

## Deferred scope

- Linux ARM64, macOS and Windows support claims.
- APK building, Gradle, project creation, arbitrary ADB shell UI and IDE
  functions.
- Waydroid, container Android and a custom emulator engine.
- Full snapshot-management UI, full device-data cloning, SDK migration and
  arbitrary disk resizing.
- Flatpak, AppImage, MSIX, DMG, Homebrew, an i18n pipeline, tray integration
  and application self-updating.
- Exhaustive parity with every flag of every official CLI.

## Capability model

Features are modeled as explicit provider capabilities rather than an undefined
"full parity" claim. Unsupported operations return unsupported, and no
substitute image, API or device name is ever chosen silently. The capability
matrix is maintained in `docs/STATUS.md` as capabilities are validated.

Basic ADB use for identity, readiness and stopping is part of the core product,
even though a general ADB frontend is out of scope.

## Guardrails for product claims

- Say "no system Java installation required", not "zero Java".
- Report total downloads and disk needs, not only launcher size.
- Standard file compatibility is a design goal with tested cases, not proof
  that every operation interoperates.
- Both classic SDK tools (`sdkmanager`, `avdmanager`) are deprecated;
  compatibility support may still be practical.
- No telemetry in Avdium does not mean an Android guest makes no network
  requests or collects no data.
- Runtime downloads can follow redirects to GitHub/CDNs; no exhaustive
  two-domain network list is claimed.
- Documented license boundaries and upstream notices are preserved; blanket
  redistribution or legal claims are not made.
- Linux ARM and Windows/macOS are future platforms.
- Flatpak is deferred for scope and integration work.
- Avoid em dashes in user-facing text.
