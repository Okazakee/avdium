# M0 validation record

Date: 2026-09-23
Host: CachyOS / Arch Linux, KDE Plasma 6, Wayland session
Scope: the focused validation matrix from `docs/AVDIUM-BOOTSTRAP.md` section 12

This file records what was actually executed, with the commands, the observed
results and the consequences. Anything not executed is listed as a gap rather
than assumed. Where a historical observation was re-probed and differed, the
difference is recorded.

Evidence levels used below: **compiled**, **automated-tested**,
**visually-inspected**, **manually-exercised**.

## 1. Environment re-probe

| Item | Observed |
| --- | --- |
| `ANDROID_HOME` | `$HOME/Android/Sdk` |
| `ANDROID_SDK_ROOT` | unset |
| `ANDROID_USER_HOME` | unset |
| `ANDROID_EMULATOR_HOME` | unset |
| `ANDROID_AVD_HOME` | unset |
| `~/.androidrc` | does not exist |
| Java | OpenJDK `21.0.11` at `/usr/bin/java` |
| Rust | `stable` = `1.98.1`; `1.98.0` installed by this session |
| `/dev/kvm` | exists, mode `0666`, group `kvm`, readable and writable |
| Emulator | `36.2.12.0` (build_id `14214601`) |
| AVD home | `$HOME/.android/avd`, 3 devices: `Pixel_9`, `Pixel_9_Pro`, `Pixel_Tablet` |
| `cmdline-tools` | **not installed** |

The historical host observations in the brief were reproduced, with two
corrections: the host runs `1.98.1` on `stable` (not `1.98.0`), and
`cmdline-tools` is absent, so `sdkmanager` and `avdmanager` are unavailable.

### Android CLI

`android-cli info --no-metrics` (exit 0):

```
sdk: $HOME/Android/Sdk
version: 1.0.16406183
```

The historical capture also showed a `launcher_version` line. This build does
not print one, so the parser treats it as optional. Recorded in
`tests/fixtures/android-cli/info.txt`.

### Android CLI embedded runtime

```
~/.android/cli/bundles/bffc6b8ca1741969f41b77766cfcbb76c42edb02/
  jre/release  -> JAVA_VERSION="25.0.4"
  jre/         -> conf/ legal/ lib/ release
  jre/bin/     -> does not exist
```

The embedded runtime is a jlink image with **no `bin` directory**, so it does
not expose a `java` executable and cannot be used to run the classic tools.
This confirms the brief's instruction not to reuse it. See
`docs/decisions/0004-managed-java.md`.

## 2. Syntax discrepancy: resolved

The brief recorded a conflict between documented
`android emulator create --profile=...` and the original report's positional
profile. The installed binary resolves it:

```
Usage: android emulator create [-h] [--list-profiles] [<profile>]
```

- The profile is a **positional** argument.
- There is **no image or API selector at all**.

Therefore Android CLI cannot create a device from a user-chosen system image,
and the documented `--profile=` form does not exist in this build. This is the
decisive input for `docs/decisions/0003-default-provider.md`.

Stopping accepts either identifier:

```
Usage: android emulator stop [-h] [<device>]
  <device>  The emulator name or serial number to stop. Optional if only one
            emulator is running
```

Complete help output for every relevant subcommand was captured under
`tests/fixtures/android-cli/help/`.

## 3. Exit codes and side effects

Measured without a pipe so the code is the tool's own:

| Command | Exit | Observation |
| --- | --- | --- |
| `android-cli info --no-metrics` | 0 | read-only |
| `android-cli emulator start --no-metrics` (no device) | 2 | usage error, nothing started |
| `android-cli emulator stop --no-metrics` (nothing running) | 0 | prints `There are currently no local emulator running` |
| `android-cli emulator frobnicate --no-metrics` | 2 | usage error |
| `android-cli emulator list --sdk=/nonexistent/sdk --no-metrics` | 0 | **ignores** `--sdk` for device listing; AVDs come from the AVD home |
| `android-cli sdk list --sdk=/nonexistent/sdk --no-metrics` | 1 | `java.nio.file.AccessDeniedException: /nonexistent` |

Two consequences:

1. Exit codes are meaningful (0 success, 1 runtime failure, 2 usage failure),
   but an error message can also appear with a zero exit, so Avdium reads the
   output as well as the code.
2. **`android sdk list` attempts to create the SDK directory.** A listing
   command can write to the filesystem, which is why the doctor keeps
   subprocess checks behind an explicit "with tool probes" action.

`/nonexistent` was not created, because the operation failed with
`AccessDeniedException` rather than succeeding. No directory outside the
temporary tree was created by any probe.

### Telemetry opt-out

`--no-metrics` is accepted by every probed invocation but is **not listed in
`--help`**. Avdium passes it on every Android CLI invocation, asserted by an
automated test (`every_invocation_passes_the_telemetry_opt_out`).

Not verified: whether the CLI still makes network requests with the flag set.
The flag's presence is verified; its effect is not.

## 4. Package repository catalog

`https://dl.google.com/android/repository/repository2-3.xml` was fetched
(409.3 KB, 317 `remotePackage` elements, 2 licenses, 4 channels) and a valid
excerpt was committed as
`tests/fixtures/catalog/repository2-3.excerpt.xml` (42 KB, 6 packages, 2 full
license texts, all 4 channels). Elements are copied verbatim; only the
surrounding document was reassembled. The excerpt passes `xmllint --noout`.

Observed facts used by the implementation:

- Channels are `channel-0` stable, `channel-1` beta, `channel-2` dev,
  `channel-3` canary.
- The newest `emulator` entry in the live document is revision `37.3.1` on
  **channel-2 (dev)**, licensed under `android-sdk-preview-license`. A stable
  selection must therefore exclude it, which the catalog selector does.
- `platform-tools` stable is `37.0.1`; `ndk;30.0.16138531` carries
  `preview=3`, so revision previews are a real case, not hypothetical.
- `platforms;android-36` has no `host-os`/`host-arch`, so host-independent
  archives are a real case.
- Checksums are SHA-1, so `Digest::is_strong()` reports `false` for them.

## 5. Checks executed

All of the following were run in the repository and passed:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --locked --all-targets -- -D warnings` | clean |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | clean |
| `cargo test --locked --all-targets` | 264 passed, 0 failed, 1 ignored |
| `cargo test --locked --all-targets --all-features` | 265 passed, 0 failed, 1 ignored |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --all-features --no-deps` | clean |

The single ignored test is `real_download_records_identity_and_size`, an
opt-in network test. It was executed manually during this session and passed:
the Android CLI launcher downloaded as `5067072` bytes with SHA-256
`1e7f2da1bb678bdb3c78a20b421a276f6b9eb639f0b6a17d46fa02f01a0b67b4`, matching
the brief's verified pin exactly.

The default test run uses no network and temporary paths. Tests never fall
through to real tools: `tests/fixtures/bin/fake-tool` records its arguments,
environment and working directory, and simulates success, failure, hangs,
prompts, carriage-return progress, invalid UTF-8, large output and grandchildren
that must be killed with the process group.

## 6. Read-only behavior against the real host

Executed against the real SDK and AVD home. Nothing was modified.

```
$ avdium --version
avdium 0.1.0-dev

$ avdium paths
sdk: $HOME/Android/Sdk (from ANDROID_HOME)
avd_home: $HOME/.android/avd (default location)
android_user_home: $HOME/.android
emulator_home: $HOME/.android
cmdline_tools: 0

$ avdium devices
Pixel_9        android-36.1 Google APIs x86_64        ok
Pixel_9_Pro    android-37.0 Google APIs PlayStore x86_64  ok
Pixel_Tablet   android-36.1 Google Play x86_64        ok

$ avdium doctor --probe
16 passed, 1 warnings, 0 failed, 0 unknown
```

The doctor reported the missing `cmdline-tools` as a warning, confirmed the
emulator version, confirmed `-accel-check` code 0, found `/dev/kvm` usable, and
found `/usr/bin/java` at `21.0.11` while stating that the selected provider does
not need it.

An inaccuracy was found and fixed during this step: the doctor previously
reported "No Java was found" when Java had never been looked for. It now
distinguishes "not checked" (unknown) from "not found" (pass or fail depending
on whether the provider needs Java). An automated test covers both.

Before and after the read-only runs, the AVD home contained exactly the same
three companion files and three data directories. No `.ini`, `config.ini`,
`userdata-qemu.img` or snapshot file was written. `avdium` itself never runs a
mutating command outside a job the user starts.

## 7. GUI

Executed, **visually-inspected**:

```
$ timeout 12 ./target/debug/avdium --log info
exit=124   # ran until the timeout, so the window stayed open
```

The process opened a window on the live Wayland session and was terminated by
the timeout, not by a failure. Accessibility traffic (`org.a11y.Status`) in the
log confirms the accesskit feature initialized.

Screenshots were captured from the running application with the `screenshot`
feature, in demo mode:

```
$ ./target/debug/avdium --demo --screenshot /tmp/avdium-shots
wrote /tmp/avdium-shots/devices.png
wrote /tmp/avdium-shots/doctor.png
wrote /tmp/avdium-shots/activity.png
wrote /tmp/avdium-shots/settings.png
wrote /tmp/avdium-shots/about.png
```

All five are valid `1120 x 760` 8-bit RGBA PNGs. Two passes produced
byte-identical sizes, so the capture is repeatable. Screens were inspected for
the no-SDK, existing-devices, broken-device, long-name and demo states.

**Renderer implemented:** a windowed renderer that requires a display server
(Wayland, X11, or a virtual display such as `Xvfb`). A truly headless renderer
is not implemented.

### Backend matrix

winit 0.30.13 selects Wayland when `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set,
and X11 when only `DISPLAY` is set. Both paths were exercised functionally by
capturing the full screenshot sequence on each:

| Session | Invocation | Result |
| --- | --- | --- |
| Native Wayland | `WAYLAND_DISPLAY=wayland-0 DISPLAY=:0` | 5 PNGs, `devices.png` 106.7 KB |
| XWayland / X11 | `env -u WAYLAND_DISPLAY DISPLAY=:0` | 5 PNGs, `devices.png` 95.9 KB |

Both runs rendered every screen and wrote valid images, and the different file
size confirms the two runs really did take different rendering paths rather
than one silently falling back.

A no-display run fails closed with an actionable message rather than a panic:

```
$ env -u WAYLAND_DISPLAY -u DISPLAY avdium --demo
avdium: the window could not be opened: winit EventLoopError: ... neither
WAYLAND_DISPLAY nor WAYLAND_SOCKET nor DISPLAY is set.
avdium: this build needs a display server. ...
avdium: the reporting subcommands (paths, tools, doctor, devices,
capabilities) work without a display.
```

**Not verified:** a plain Xorg session (no Wayland compositor), because `Xvfb`
is not installed and installing a package would be a host change outside this
task's authorization. XWayland exercises the X11 backend but not an Xorg
server's own input, scaling or window-manager behavior.

**Not verified:** whether the X11 `WM_CLASS` and the Wayland `app_id` actually
match `dev.okazakee.Avdium` at runtime. Both are set from one constant in
`src/main.rs` and the desktop entry uses `StartupWMClass=dev.okazakee.Avdium`,
but the association was not inspected with `xprop` or a compositor tool.

**Not verified:** fractional scaling and HiDPI on either backend.

## 8. Not collected, and what each gap blocks

| Gap | Blocks |
| --- | --- |
| Classic tools (`sdkmanager`, `avdmanager`) are not installed anywhere on this host | The M2 selected-image creation path, and validation of every argument array in `providers/cmdline_tools.rs`. That module is documented as unvalidated in its own doc comment. |
| No Java runtime was matched to a pinned classic-tools build | The managed-Java decision. No Temurin version or checksum is recorded anywhere in the repository, because the Adoptium asset resolution was not performed. |
| No disposable AVD was created | Real start, readiness, cold boot, stop, wipe and delete. `docs/decisions/0005-runtime-lifecycle.md` records the intended commands; only the read-only probes were executed. |
| `.androidrc` does not exist on this host | The ambient-configuration precedence check. Avdium passes an explicit `--sdk` and a consistent environment, but whether that neutralizes every ambient flag is unverified. |
| Fresh CLI bootstrap and offline/repeat invocation were not performed | Whether the CLI reuses payloads, makes network checks, or starts offline. Avdium never runs `android update`. |
| The packaging pipeline was not executed | Every packaging claim. No package has been built, installed, removed or published. |
| Which windowing backend winit selected | An X11-specific support claim. |
| `android sdk list` output for the available (not installed) sections | The available-package picker. Only the installed section was captured in full. |

## 9. Test layers and a fixture defect that was fixed

The repository originally had 399 unit tests across 26 modules but **no
integration tests** (`tests/` held only fixtures) and **no committed
end-to-end test**. The lifecycle verification had been done by hand.

Added:

- `tests/common/mod.rs`: a disposable sandbox with an SDK, an AVD home, a user
  home and Avdium's own XDG directories under one temporary tree, with an empty
  directory on `PATH` so no test can reach a real `android`, `sdkmanager`,
  `avdmanager`, `emulator`, `adb` or `java`.
- `tests/cli_reporting.rs`: 28 integration tests running the built binary as a
  black box.
- `tests/e2e_lifecycle.rs`: 2 opt-in tests reproducing the disposable-SDK
  lifecycle.

Building the integration layer exposed a defect **in the fixture**, which is
recorded here because it is the kind of thing that gets papered over:

**Symptom.** The fake tool, executed as a subprocess, intermittently produced no
output at all with exit status 0. Roughly one run in four under parallel load.

**Wrong turns.** Five hypotheses were tested and discarded: shell quoting in a
generated wrapper script, a `basename` call that the deliberately empty `PATH`
made unavailable (a real bug, fixed), write-then-execute races, copies versus
symlinks for the fake tool, and concurrency itself. A retry was added at one
point and then **removed**, because a retry would have turned the suite green
while leaving the defect in place. Four tests were briefly quarantined with a
recorded reason.

**Actual cause.** The fake tool read its scenario from a file written moments
earlier, and that read sometimes returned only the first line. The measurement
that settled it was the tool reporting how many directives it had actually
read, plus the byte count in the failure: 18 bytes, exactly one directive.

**Fix.** The fixture now generates each fake tool with its scenario **inlined as
literal shell statements** at fixture-creation time, so the tool reads nothing at
run time. This removes the class of problem rather than masking it. An
unsupported directive is now a hard error, so a fixture cannot silently do
nothing again.

**Result.** 28 integration tests, none ignored, **0 failures in 60 consecutive
runs** under parallel load.

## 9. End-to-end lifecycle against a disposable SDK

The first real exercise of the whole chain. Everything ran against a
**disposable SDK** at `/tmp/avdium-e2e` with its own AVD home, using symlinks to
the real emulator, platform-tools and system-images as read-only inputs. The
user's own SDK and AVD home were never written to. The disposable tree was
removed afterwards.

### Provisioning, with a real download

`avdium provision install --pin cmdline-tools --sdk /tmp/avdium-e2e/sdk`:

- Fetched 172.7 MiB from `commandlinetools-linux-16111833_latest.zip`.
- Verified SHA-1 `e025545c62a8e64c7559119566a569fb1dec5f60` against Google's
  published metadata. The plan and the tool both label this digest as weak and
  as coming from the publisher, not from this project.
- Extracted safely, resolved the archive's real top-level layout, validated
  that `sdkmanager` and `avdmanager` were present.
- Promoted to the **versioned** `cmdline-tools/23.0`, refusing to touch a
  `latest` directory, with executable bits preserved (`0755` on the launchers).
- Wrote a record naming the URL, expected digest, its provenance, the actual
  digest and the install path.

`avdium provision plan --create-devices` on the same machine identified exactly
one missing step and reused everything else: Android CLI, the emulator,
platform-tools, three installed system images, and the Java already present at
`/usr/bin/java` (`21.0.11`). Nothing already installed was scheduled for
upgrade.

### Creation with a chosen image

Android CLI cannot do this, so it went through the classic adapter:

```
avdmanager create avd -n Avdium_E2E_101996 -k system-images;android-36.1;google_apis;x86_64
```

The device was created in the disposable AVD home, from the image that was
asked for, with no substitution. `avdium create --dry-run` printed the same
argument array first. Avdium's own discovery then listed it correctly.

### Runtime lifecycle

| Step | Result |
| --- | --- |
| Launch (`-no-snapshot-load -no-window`) | process started |
| ADB serial detected | 3 s |
| `sys.boot_completed` = 1 | 31 s |
| Identity (`adb -s SERIAL emu avd name`) | `Avdium_E2E_101996`, matching the launched device |
| Guest properties | model `sdk_gphone64_x86_64`, API 36, release 16 |
| Stop (`adb -s SERIAL emu kill`) | `OK: killing emulator, bye bye` |
| `adb devices` after stop | empty |
| Wipe (`-wipe-data`) | device booted again and stopped cleanly |
| Delete (`avdmanager delete avd`) | `AVD 'Avdium_E2E_101996' deleted.` |
| Discovery after delete | device no longer listed |

This validates the exact commands the runtime layer builds, the serial-to-AVD
mapping it relies on, and the readiness signal it polls.

### Isolation check

After the run: no stray emulator processes, the real AVD home still contained
exactly the same three companions with their original mtimes (January, April
and August), and the real SDK still had no `cmdline-tools` directory at all.
The disposable tree was deleted.

**Still not verified:** the same lifecycle driven from the GUI. The commands are
proven, but the buttons that submit them have not been exercised against a real
device.

## 9. Release build and dynamic dependencies

`cargo build --locked --release` succeeded (329 crates, 1m04s wall). The result
is a 13.4 MB stripped `ELF 64-bit LSB pie executable` that reports
`avdium 0.1.0-dev`.

`ldd` on the release binary shows the direct ELF dependencies are only:

```
libc.so.6
libgcc_s.so.1
libm.so.6
```

Every GUI library is loaded at runtime and is therefore invisible to ELF
inspection. Each of the following was checked with `ldd` and is **not** linked:

```
libGL.so.1  libEGL.so.1  libwayland-client.so.0  libxkbcommon.so.0
libxkbcommon-x11.so.0  libX11.so.6  libxcb.so.1  libfontconfig.so.1
```

This is why `native-packages.yaml` declares them under `libraries:` instead of
relying on nFPM's automatic dependency detection. The list is now evidence-based
rather than guessed, but the per-distribution package **names** are still
unvalidated: no package has been built or installed. The release build also
confirms that a Rust binary is not dependency-free, and that Java and QEMU are
correctly absent from the dependency set.

## 10. Fixtures captured in this session

All under `tests/fixtures/`, from the installed tools on this host:

- `android-cli/info.txt`, `emulator-list.txt`, `emulator-list-long.txt`,
  `profiles.txt`, `sdk-list.txt`
- `android-cli/help/` for `info`, `emulator start|create|stop|remove|list`,
  `sdk list|install|remove|update`
- `emulator/version.txt`, `list-avds.txt`, `accel-check.txt`
- `adb/devices-l.txt` (no devices attached)
- `catalog/repository2-3.excerpt.xml`

`tests/fixtures/bin/fake-tool` is constructed, not captured, and is documented
as such in its own header comment.
