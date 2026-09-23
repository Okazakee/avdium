# Roadmap

This file owns the ordered plan and the goals behind the ordering. It is not a
restatement of the brief: `PRODUCT.md` owns scope, `docs/STATUS.md` owns the
current state, and `docs/VALIDATION.md` owns executed evidence.

## Goals that shape the ordering

1. **Both tool families, fully.** Android CLI is the current tool and the
   classic tools (`sdkmanager`, `avdmanager`) are deprecated, but both are
   supported to the limit of what each can actually do. A capability that only
   one family has must still be reachable. Capabilities are reported per
   provider, never as an averaged "works" claim.
2. **Wayland and X11/XWayland.** The application must run on a native Wayland
   session, on XWayland, and on a plain Xorg session. Where the backend affects
   behavior (window identity, file dialogs, fractional scaling), the difference
   is handled rather than ignored.
3. **No manual setup.** Opening Avdium on a machine with nothing installed
   should reach a working emulator with as few decisions as possible. The tool
   detects what is already present, reuses it, and only then offers to fetch
   what is missing. A system Java install is never required; a compatible Java
   that already exists is reused.
4. **Stability before polish.** Nothing is polished, packaged for more
   architectures, or released until the core lifecycle is correct and
   validated.
5. **Nothing destructive without evidence.** Every mutation rechecks identity,
   activity and external changes immediately before it commits.

## Order of work

Stability work comes first, in this order. Each milestone is complete only when
its real-world check has actually been executed.

### S1. Existing-SDK manager, validated end to end (M1 completion)

Already implemented: discovery, device model, doctor, process infrastructure,
operation coordinator, UI surfaces, demo mode.

Remaining: create a disposable AVD with a unique name and a dedicated data
directory, then exercise start, readiness, cold boot, stop and wipe against it,
and record the emulator and image versions. Until this is executed, M1 is
implemented but not validated.

### S2. Provisioning and creation (M2)

This is the milestone that removes manual setup.

- Catalog fetching and caching with freshness information, bounded input, and
  offline tolerance: a catalog failure must never prevent listing or starting
  installed devices.
- A real consent flow. Exact license text is shown before the action that
  requires it, the decision is recorded against the exact text identity, and
  only the answers covered by that decision are sent. No unbounded `yes` pipe.
  Consent already validly recorded by installed tooling is respected.
- Tool installation: download into private staging, verify size and an
  independently obtained checksum, extract safely, validate, then promote on
  the destination filesystem. Never overwrite `cmdline-tools/latest` blindly.
- Runtime selection: reuse an already-compatible Java when one exists; offer a
  private Temurin runtime only when none does, resolved to an exact release and
  published checksum.
- Package operations through the selected provider, with actual resulting state
  verified afterwards rather than assumed.
- Selected-image device creation. Android CLI cannot do this, so the classic
  `avdmanager` adapter is the path, and the additional compatibility-tool
  download is explained rather than hidden.

Exit criteria: on a machine with no SDK tooling and no Java, Avdium provisions
the tools and a chosen image, creates a disposable device, boots it, and stops
it, with cancellation, failure and consent refusal all behaving correctly.

### S3. Safe mutation (M3)

- Config diffs with a human-readable summary before anything is written.
- Conflict checks re-run immediately before committing.
- Supported edits: RAM, CPU cores, keyboard, graphics mode, cameras, boot
  behavior, and validated resolution, density and network simulation.
- Wipe through the emulator operation, never by deleting a single image file.
- Dependency-aware package removal, blocked while a referenced device runs.
- Validated delete that refuses ambiguous or dangerous destinations.

Exit criteria: a device boots after settings changes; no mutation happens while
a device is running; unsafe paths and stale edits are refused; wipe and delete
are exercised on a disposable device; interrupted operations have a documented
recovery route.

### S4. Wayland and X11/XWayland matrix

Verify and, where needed, fix:

- Native Wayland session.
- XWayland inside a Wayland session.
- Plain Xorg session.
- Window identity matches the desktop entry on both paths: the Wayland `app_id`
  and the X11 `WM_CLASS` must both be `dev.okazakee.Avdium`, and the desktop
  entry must associate correctly.
- The native folder picker, including the fallback when no XDG portal is
  running. Manual path entry must always remain usable.
- Fractional scaling and HiDPI on both backends.

Exit criteria: all three sessions are exercised and the results recorded, with
the backend that was actually selected stated rather than assumed.

### S5. UI polish

Only after S1 to S4 are stable. This is the "make it good" pass, not a
correctness pass.

- Density, spacing and type-scale review against the token set.
- Keyboard navigation and focus order across every surface.
- Accessibility labels for controls, tables and status indicators.
- Empty, error and long-content states reviewed in both light and dark themes.
- Log viewer ergonomics: filtering, follow, and copy.

### S6. Packaging

- Validate `native-packages.yaml` on each supported distribution, confirming
  the runtime dependency names, which are currently evidence-based but
  unvalidated.
- Build and install DEB, RPM and the release tarball in clean containers, then
  run `avdium --version`, check runtime libraries and assets, and uninstall
  while preserving user settings.
- Choose and document the minimum glibc and distribution baseline.
- Stage the AUR binary recipe and generate valid `.SRCINFO`.

### S7. Multi-architecture support

Deferred until last, and blocked on real upstream availability.

- Linux ARM64. The official emulator publishes no Linux aarch64 archive, so
  this cannot be a full-support claim. The realistic outcome is that Avdium
  builds and runs, manages an SDK, and reports the emulator as unavailable with
  a clear reason.
- macOS and Windows are separate efforts with their own packaging, window
  identity and runtime-discovery work. No partial support claim is made.

## What is deliberately not planned

APK building, Gradle, project scaffolding, an arbitrary ADB shell frontend,
Waydroid or container Android, a custom emulator engine, full snapshot
management, full device-data cloning, SDK migration, arbitrary disk resizing,
Flatpak, AppImage, tray integration, i18n, and application self-updating. See
`PRODUCT.md`.

## Cross-cutting rules

These apply to every milestone above.

- A blocking operation never runs on the UI thread.
- A tool invocation passes an argument array, never a reconstructed command.
- Unsupported operations say so. No substitute image, API level or device name
  is ever chosen silently.
- Running devices are left running when the window closes.
- Avdium has no telemetry, and it passes supported opt-outs to the tools it
  invokes. It never claims the Android guest makes no network requests.
- Avdium never runs privileged commands. Group membership is one possible KVM
  fix, never presented as the diagnosis.
