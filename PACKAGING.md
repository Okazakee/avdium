# Packaging and release

This document owns packaging and release mechanics. It does not claim any
artifact has been published. At bootstrap the pipeline is staged and unverified
because no release destination exists yet.

## Principles

- `native-packages.yaml` is the single source of package metadata.
- Linux x86_64 only for v0.1.
- No Google tools, SDK payloads or system images are redistributed.
- Java is not a prerequisite for opening Avdium.
- QEMU is not a package dependency; the Android emulator supplies its own
  engine.
- `publish = false` in `Cargo.toml` and the inactive release workflow are
  intentional until a destination is established.

## Artifacts

```text
avdium-vVERSION-x86_64-unknown-linux-gnu.tar.gz
avdium-vVERSION-x86_64-unknown-linux-gnu/
  avdium
  README.md
  LICENSE
  packaging/applications/dev.okazakee.Avdium.desktop
  packaging/icons/dev.okazakee.Avdium.svg
```

Native packages place:

- the binary at `/usr/bin/avdium`;
- the desktop entry under `/usr/share/applications/`;
- the icon under `/usr/share/icons/hicolor/scalable/apps/`;
- license and readme in conventional package-specific locations.

The desktop entry uses `Exec=avdium` and `StartupWMClass=dev.okazakee.Avdium`.
The Wayland `app_id` and X11 `WM_CLASS` are set to the same value from
`src/app.rs` through the egui viewport, then tested.

## Pinned tooling

- native-packages `0.7.0`
- nFPM `2.47.0`

Confirm the current schema and command syntax from the pinned native-packages
source before changing templates. Do not paste a guessed reusable workflow
interface.

## Runtime dependencies

Package dependencies are derived from Avdium's actual build and runtime
requirements, not copied from another project. Dynamically loaded GUI
dependencies are probed where ELF linkage misses them (for example GL/EGL and
Wayland/X11 client libraries).

Choose and document a minimum glibc/distro baseline before the first release.
A build on one distribution does not automatically support older ones.

## Release workflow (staged, not executed)

The release workflow validates version consistency, requires the tag to point
at a commit reachable from the intended mainline, produces checksums and
attestations, and writes release notes describing implemented and validated
behavior only. Third-party actions are pinned by full commit SHA, permissions
are minimized, and signing or AUR secrets are never exposed to untrusted jobs.

External publication requires an established destination and explicit user
authorization through the actual workflow.

## AUR

`packaging/arch/avdium-bin/PKGBUILD.in` is a template. It must use release
checksums and generate valid `.SRCINFO`. It is staged only; nothing is pushed.
