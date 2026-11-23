#!/usr/bin/env bash
# Packs a built `scrawl` into an AppImage with GStreamer and its plugins (screen capture,
# H.264 encoding, decoding) inside, so a movie records on any desktop:
#
#   ./scripts/linux/appimage.sh [path/to/scrawl]   # default target/release/scrawl
#
# Writes target/appimage/Scrawl-<arch>.AppImage. Needs the GStreamer plugin packages installed
# (gstreamer1.0-plugins-{base,good,bad,ugly}, gstreamer1.0-pipewire) and patchelf; linuxdeploy and its
# GStreamer plugin are downloaded into target/appimage/tools the first time.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
binary="$(realpath "${1:-$root/target/release/scrawl}")"
arch="$(uname -m)"
out="$root/target/appimage"
tools="$out/tools"
appdir="$out/AppDir"
mkdir -p "$tools"
rm -rf "$appdir"

# Pinned tools, each checked against its SHA-256 before it runs: what packs the release is
# never whatever a moving tag or branch serves today. To update, change the tag or commit and
# its hashes (GitHub shows a release asset's sha256 as its digest).
linuxdeploy_tag="1-alpha-20251107-1"
case "$arch" in
  x86_64) linuxdeploy_sha256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d" ;;
  aarch64) linuxdeploy_sha256="620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff" ;;
  *) echo "No pinned linuxdeploy for $arch" >&2; exit 1 ;;
esac
gstreamer_plugin_commit="432af639725bc8b99df12351e86bb2e8a6b3585f"
gstreamer_plugin_sha256="d1cf90701047aa9583c274bb66dd82db352255374fbfb8f3c4c89a05fe23bce0"

fetch() {
  local url="$1" name="$2" sha256="$3"
  if ! echo "$sha256  $tools/$name" | sha256sum --check --status 2>/dev/null; then
    curl -fsSL -o "$tools/$name.part" "$url"
    if ! echo "$sha256  $tools/$name.part" | sha256sum --check --status; then
      echo "$name from $url does not match its pinned SHA-256" >&2
      rm -f "$tools/$name.part"
      exit 1
    fi
    mv "$tools/$name.part" "$tools/$name"
  fi
  chmod +x "$tools/$name"
}
fetch "https://github.com/linuxdeploy/linuxdeploy/releases/download/$linuxdeploy_tag/linuxdeploy-$arch.AppImage" \
  linuxdeploy "$linuxdeploy_sha256"
fetch "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gstreamer/$gstreamer_plugin_commit/linuxdeploy-plugin-gstreamer.sh" \
  linuxdeploy-plugin-gstreamer.sh "$gstreamer_plugin_sha256"

multiarch="$(gcc -print-multiarch 2>/dev/null || echo "$arch-linux-gnu")"
export GSTREAMER_PLUGINS_DIR="/usr/lib/$multiarch/gstreamer-1.0"
export GSTREAMER_HELPERS_DIR="/usr/lib/$multiarch/gstreamer1.0/gstreamer-1.0"
export GSTREAMER_INCLUDE_BAD_PLUGINS=1
# CI runners have no FUSE.
export APPIMAGE_EXTRACT_AND_RUN=1
export PATH="$tools:$PATH"

# These modules are dlopen'd, not DT_NEEDED dependencies: linuxdeploy cannot
# discover them from scrawl. Stage every tier next to libtranscribe in usr/lib.
libraries=()
for library in "$(dirname "$binary")"/libggml-cpu*.so; do
  [[ -f "$library" ]] || { echo "Missing transcribe runtime: $library" >&2; exit 1; }
  libraries+=(--library "$library")
done

cd "$out"
LDAI_OUTPUT="Scrawl-$arch.AppImage" linuxdeploy \
  --appdir "$appdir" \
  --executable "$binary" \
  "${libraries[@]}" \
  --executable "$(command -v wl-copy)" \
  --desktop-file "$root/packaging/linux/dev.kitlangton.scrawl.desktop" \
  --icon-file "$root/packaging/linux/dev.kitlangton.scrawl.png" \
  --plugin gstreamer \
  --output appimage
echo "$out/Scrawl-$arch.AppImage"
