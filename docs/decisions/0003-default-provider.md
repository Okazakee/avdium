# Decision: default SDK provider

Date: 2026-09-23
Status: accepted for read-only operations; provisioning decision pending M2

## Context

Android CLI (`android`) and the deprecated classic tools (`sdkmanager`,
`avdmanager`) are both candidates. The brief prefers Android CLI for new SDK
provisioning and keeps classic tools as a compatibility and advanced-creation
provider.

## Evidence collected on this host

`~/.android/bin/android-cli` reports version `1.0.16406183`. Complete help
output was captured for every relevant subcommand (see
`docs/VALIDATION.md`). Two points decide the split:

1. `android emulator create [-h] [--list-profiles] [<profile>]` takes a
   **positional profile** and has **no image or API selector**. Android CLI
   cannot create a device from a user-chosen system image.
2. `android emulator stop [<device>]` documents the positional as
   "the emulator name or serial number", so both identifiers are accepted.

No `cmdline-tools` directory exists under the local SDK, so `sdkmanager` and
`avdmanager` are not installed here and could not be exercised.

## Decision

- Read-only discovery, listing, device listing and device start/stop use
  Android CLI where it is available, and the emulator/ADB binaries directly for
  runtime behavior.
- Package listing uses Android CLI as the primary source, with classic
  `sdkmanager` as a fallback when only classic tools are present.
- **Selected-image creation requires the classic `avdmanager` adapter.** Android
  CLI is not capable of it. This is a declared capability gap, not a silent
  substitution.
- Every Android CLI invocation passes `--no-metrics`. The flag is accepted by
  the installed binary even though it is not listed in `--help`.

## Consequences

M2 cannot be completed with Android CLI alone. A classic-tools adapter plus a
validated Java runtime is required, and the local host currently has neither
installed. The milestone stays implemented but unvalidated until that path is
exercised, per the brief's completion rule.
