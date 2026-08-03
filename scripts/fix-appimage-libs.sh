#!/bin/bash
# Removes the bundled Wayland client libraries from a Tauri AppImage.
#
# Why this exists
# ---------------
# linuxdeploy bundles the build machine's libwayland-client (Ubuntu 24.04 ships
# wayland 1.22) and the AppDir comes first in the library search path. On any
# distribution with wayland >= 1.23 and a current Mesa -- Arch, Manjaro,
# Fedora 41+, openSUSE Tumbleweed -- the system's EGL vendor library then fails
# to resolve a symbol it needs from the newer libwayland:
#
#   /usr/lib/libEGL_mesa.so.0: symbol lookup error: undefined symbol:
#   wl_fixes_interface (fatal)
#
# libglvnd is left without a vendor, eglInitialize returns EGL_BAD_PARAMETER,
# and WebKit aborts its renderer with "Could not create default EGL display".
# The GTK window opens and stays blank -- the app looks broken while every part
# of it is fine.
#
# The bundled copies are not needed. The AppRun hook forces GDK_BACKEND=x11,
# and on a Wayland session the system's libwayland has to be used anyway,
# because it must match the compositor and the graphics driver.
#
# Usage: fix-appimage-libs.sh <path-to-.AppImage>
set -euo pipefail

# Pinned to an immutable release rather than `continuous`, so a CI run cannot
# silently start using a different packer.
APPIMAGETOOL_URL="https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage"
APPIMAGETOOL_SHA256="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0"

# Libraries whose bundled copy breaks the host's graphics stack. Keep this list
# minimal: everything removed here has to exist on the target system.
REMOVE_GLOBS=(
  "libwayland-client.so*"
  "libwayland-cursor.so*"
  "libwayland-egl.so*"
  "libwayland-server.so*"
)

appimage="${1:-}"
if [ -z "$appimage" ]; then
  echo "usage: $0 <path-to-.AppImage>" >&2
  exit 2
fi
if [ ! -f "$appimage" ]; then
  echo "error: no such file: $appimage" >&2
  exit 1
fi

appimage="$(readlink -f "$appimage")"
name="$(basename "$appimage")"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "==> $name"

chmod +x "$appimage"
# --appimage-extract needs no FUSE, which GitHub runners do not provide.
(cd "$workdir" && "$appimage" --appimage-extract >/dev/null)
appdir="$workdir/squashfs-root"
if [ ! -d "$appdir" ]; then
  echo "error: extraction produced no squashfs-root" >&2
  exit 1
fi

removed=0
for glob in "${REMOVE_GLOBS[@]}"; do
  while IFS= read -r lib; do
    echo "    removing $(basename "$lib")"
    rm -f "$lib"
    removed=$((removed + 1))
  done < <(find "$appdir" -name "$glob" -type f 2>/dev/null)
done

# A future linuxdeploy or a change in its exclude list may stop bundling these.
# That is the desired end state, so report it instead of failing.
if [ "$removed" -eq 0 ]; then
  echo "    nothing to remove; the bundle already relies on the system libraries"
  exit 0
fi

tool="$workdir/appimagetool"
if command -v appimagetool >/dev/null 2>&1; then
  tool="$(command -v appimagetool)"
else
  echo "    fetching appimagetool"
  curl -fsSL "$APPIMAGETOOL_URL" -o "$tool"
  actual="$(sha256sum "$tool" | cut -d' ' -f1)"
  if [ "$actual" != "$APPIMAGETOOL_SHA256" ]; then
    echo "error: appimagetool checksum mismatch (expected $APPIMAGETOOL_SHA256, got $actual)" >&2
    exit 1
  fi
  chmod +x "$tool"
fi

runtime_args=()
# Reuse the runtime the original AppImage already carries. Left to itself,
# appimagetool downloads one from a `continuous` release at pack time, which
# would mean an unpinned binary in the middle of a release build.
offset="$("$appimage" --appimage-offset 2>/dev/null || true)"
if [ -n "$offset" ] && [ "$offset" -gt 0 ] 2>/dev/null; then
  if dd if="$appimage" of="$workdir/runtime" bs="$offset" count=1 status=none 2>/dev/null; then
    runtime_args=(--runtime-file "$workdir/runtime")
    echo "    reusing the original runtime ($offset bytes)"
  fi
fi
if [ "${#runtime_args[@]}" -eq 0 ]; then
  echo "    warning: could not reuse the original runtime; appimagetool will fetch one" >&2
fi

echo "    repacking"
# ARCH is required by appimagetool; -n skips AppStream validation, which a
# linuxdeploy AppDir has no metadata for.
ARCH=x86_64 "$tool" --appimage-extract-and-run -n "${runtime_args[@]}" "$appdir" "$workdir/$name" >/dev/null

if [ ! -s "$workdir/$name" ]; then
  echo "error: repacking produced no output" >&2
  exit 1
fi

mv "$workdir/$name" "$appimage"
chmod +x "$appimage"
echo "    done: $removed library/libraries removed, $(du -h "$appimage" | cut -f1) total"
