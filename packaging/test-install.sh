#!/usr/bin/env bash
# Install a built Avdium package in a clean container, verify it runs, verify
# that runtime libraries and packaged assets are present, then uninstall and
# check that user settings survive.
#
# Usage: packaging/test-install.sh <container-image> <native-packages-dir>
#
# This script is staged. It has not been executed against a real package yet;
# see docs/VALIDATION.md. It must never run against the developer's own
# machine's package database.
set -euo pipefail

IMAGE=${1:?usage: test-install.sh <container-image> <native-packages-dir>}
PACKAGES_DIR=${2:?usage: test-install.sh <container-image> <native-packages-dir>}

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required to run install tests" >&2
  exit 1
fi

PACKAGES_DIR=$(cd "$PACKAGES_DIR" && pwd)

case "$IMAGE" in
  *fedora*) FORMAT=rpm ;;
  *) FORMAT=deb ;;
esac

PACKAGE=$(find "$PACKAGES_DIR" -type f -name "*.${FORMAT}" | head -n 1)
if [ -z "$PACKAGE" ]; then
  echo "no .${FORMAT} package found under $PACKAGES_DIR" >&2
  exit 1
fi
PACKAGE_NAME=$(basename "$PACKAGE")

echo "== $IMAGE: installing $PACKAGE_NAME"

docker run --rm \
  -v "$PACKAGES_DIR:/packages:ro" \
  -e PACKAGE_NAME="$PACKAGE_NAME" \
  -e FORMAT="$FORMAT" \
  "$IMAGE" bash -euo pipefail -c '
    install_package() {
      case "$FORMAT" in
        deb) apt-get update -qq && apt-get install -y -qq "/packages/$PACKAGE_NAME" ;;
        rpm) dnf install -y -q "/packages/$PACKAGE_NAME" ;;
      esac
    }
    remove_package() {
      case "$FORMAT" in
        deb) apt-get remove -y -qq avdium ;;
        rpm) dnf remove -y -q avdium ;;
      esac
    }

    install_package

    test -x /usr/bin/avdium
    avdium --version
    avdium --help > /dev/null

    test -f /usr/share/applications/dev.okazakee.Avdium.desktop
    test -f /usr/share/icons/hicolor/scalable/apps/dev.okazakee.Avdium.svg

    # Dynamically loaded GUI libraries are the ones ELF linkage misses.
    for lib in libGL.so.1 libEGL.so.1 libwayland-client.so.0 libxkbcommon.so.0; do
      ldconfig -p | grep -q "$lib" || { echo "missing runtime library: $lib" >&2; exit 1; }
    done

    # Java must not be a prerequisite for opening Avdium.
    if command -v java >/dev/null 2>&1; then
      echo "note: java is present in this image; the check is not proving absence" >&2
    fi

    # A user settings file must survive uninstall.
    mkdir -p /root/.config/avdium
    printf "{\"schema\":1}\n" > /root/.config/avdium/settings.json

    remove_package

    test ! -e /usr/bin/avdium
    test -f /root/.config/avdium/settings.json

    echo "install test passed"
  '
