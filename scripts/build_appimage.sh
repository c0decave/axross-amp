#!/usr/bin/env bash
# Build axross-amp's AppImage from a release binary.
#
# Usage: scripts/build_appimage.sh BINARY VERSION OUTDIR [TOOLS]
#   BINARY   target/release/axross-amp (built on an old enough glibc:
#            the release workflow builds on Ubuntu 22.04, glibc 2.35)
#   VERSION  0.14.0
#   OUTDIR   where axross-amp-VERSION-x86_64.AppImage goes
#   TOOLS    where the pinned appimagetool and runtime are (fetched there,
#            verified by SHA-256, by scripts/fetch_appimagetool.sh)
#
# The AppDir holds the binary, the desktop entry and the icon from
# packaging/. No library is bundled: the binary needs glibc 2.35 or later
# and ALSA's libasound.so.2 (every desktop has it); it opens X11 / Wayland
# and Vulkan / GL at run time from the system, as any desktop program.
# AppDirOnly=1 builds the AppDir alone (no download: for a local check).
set -euo pipefail

BINARY="${1:?usage: build_appimage.sh BINARY VERSION OUTDIR [TOOLS]}"
VERSION="${2:?version}"
OUTDIR="${3:?outdir}"
TOOLS="${4:-$OUTDIR/tools}"
HERE="$(cd -- "$(dirname -- "$0")/.." && pwd)"

APPDIR="$OUTDIR/axross-amp.AppDir"
rm -rf -- "$APPDIR"
mkdir -p -- "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps"
install -m 0755 -- "$BINARY" "$APPDIR/usr/bin/axross-amp"
install -m 0644 -- "$HERE/packaging/axross-amp.desktop" "$APPDIR/usr/share/applications/axross-amp.desktop"
install -m 0644 -- "$HERE/packaging/axross-amp.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/axross-amp.svg"
# (appimagetool wants the entry and the icon at the AppDir's top as well)
cp -- "$APPDIR/usr/share/applications/axross-amp.desktop" "$APPDIR/axross-amp.desktop"
cp -- "$APPDIR/usr/share/icons/hicolor/scalable/apps/axross-amp.svg" "$APPDIR/axross-amp.svg"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/axross-amp" "$@"
RUN
chmod 0755 -- "$APPDIR/AppRun"
echo ">>> AppDir: $APPDIR"
if [ "${AppDirOnly:-0}" = 1 ]; then
    exit 0
fi

"$HERE/scripts/fetch_appimagetool.sh" "$TOOLS/appimagetool-x86_64.AppImage"
"$HERE/scripts/fetch_appimagetool.sh" "$TOOLS/runtime-x86_64" --runtime
OUT="$OUTDIR/axross-amp-$VERSION-x86_64.AppImage"
# (extracted, not mounted: no FUSE needed on the build machine)
( cd -- "$TOOLS" && rm -rf squashfs-root && ./appimagetool-x86_64.AppImage --appimage-extract >/dev/null )
ARCH=x86_64 "$TOOLS/squashfs-root/AppRun" --no-appstream \
    --runtime-file "$TOOLS/runtime-x86_64" "$APPDIR" "$OUT"
echo ">>> $OUT"
