# Decision: emulator runtime lifecycle

Date: 2026-09-23
Status: accepted

## Context

Android CLI's `emulator start` "will return when the emulator is fully started
and ready to use", which means the wrapper is not a handle to the long-lived
emulator process. Avdium needs to start, observe, stop and clean up devices
without depending on a wrapper that has already exited.

## Decision

- Launch devices through the emulator executable directly, with an argument
  array, and track the child process group that Avdium owns.
- Use `-no-snapshot-load` for cold boot. Do not use a raw `-cold-boot` flag;
  it is not verified.
- Leave graphics at `auto`. Do not default to `swiftshader_indirect`; it is
  deprecated from emulator 36.4.9. Do not assume `software` is universally
  supported by the installed build.
- Determine readiness from ADB: `adb devices -l` showing `device` is not a
  boot-complete signal. Poll `getprop sys.boot_completed` with a bounded
  timeout, and keep the collected output for failure diagnosis.
- Map serials back to AVD identity with an emulator/ADB query rather than
  trusting a reused serial.
- Discover externally launched devices in addition to Avdium-owned ones.
- Closing the GUI leaves running emulators running. Stopping is an explicit
  action.
- Never use `adb kill-server` as routine recovery; the server may serve other
  tools and physical devices.

## Evidence

The runtime commands in the brief's section 9 are taken as intended operations.
`emulator -list-avds`, `emulator -accel-check` and `emulator -version` were
executed successfully on this host. Start/stop/readiness against a real
disposable device has **not** been executed and is recorded as a validation
gap.

## Consequences

Runtime lifecycle is implemented and unit-tested against fake tools. Real
device boot, readiness and stop remain unvalidated until a disposable AVD is
created and used.
