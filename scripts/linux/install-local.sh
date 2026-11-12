#!/usr/bin/env bash
# Builds Scrawl and installs it for this user, without an AppImage: for a distribution that
# ships GStreamer and its plugins itself (Arch, Omarchy). Then restarts the tray app.
#
#   ./scripts/linux/install-local.sh            # cargo build --release, then install
#   ./scripts/linux/install-local.sh --no-build # install what target/release holds
#
# Puts the binary and transcribe.cpp's libraries (found beside it through $ORIGIN) in
# ~/.local/lib/scrawl, links ~/.local/bin/scrawl to it, and adds the app launcher entry and icon.
# Arch packages: base-devel cmake ninja clang gst-plugins-{base,good,bad,ugly} gst-plugin-pipewire
# gst-libav x264 wl-clipboard vulkan-icd-loader.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
lib="$HOME/.local/lib/scrawl"
bin="$HOME/.local/bin"
apps="$HOME/.local/share/applications"
icons="$HOME/.local/share/icons/hicolor/256x256/apps"

if [[ "${1:-}" != "--no-build" ]]; then
  (cd "$root" && cargo build --release)
fi
release="$root/target/release"
[[ -x "$release/scrawl" ]] || { echo "No $release/scrawl; build it first" >&2; exit 1; }

mkdir -p "$lib" "$bin" "$apps" "$icons"
# Copy then rename: the running app holds the old files open, and a rename leaves them be.
for file in "$release/scrawl" "$release"/*.so*; do
  [[ -f "$file" ]] || continue
  name="$(basename "$file")"
  cp -f "$file" "$lib/.$name.new"
  mv -f "$lib/.$name.new" "$lib/$name"
done
ln -sfn "$lib/scrawl" "$bin/scrawl"

install -m 644 "$root/packaging/linux/dev.kitlangton.scrawl.png" "$icons/dev.kitlangton.scrawl.png"
sed "s|^Exec=.*|Exec=$lib/scrawl app|" "$root/packaging/linux/dev.kitlangton.scrawl.desktop" \
  > "$apps/dev.kitlangton.scrawl.desktop"
update-desktop-database "$apps" >/dev/null 2>&1 || true

# Restart the tray app from the new binary, outside this shell's process group.
pkill -x scrawl 2>/dev/null || true
for _ in $(seq 20); do pgrep -x scrawl >/dev/null || break; sleep 0.1; done
if command -v uwsm-app >/dev/null 2>&1; then
  setsid uwsm-app -- "$lib/scrawl" app >/dev/null 2>&1 < /dev/null &
else
  setsid "$lib/scrawl" app >/dev/null 2>&1 < /dev/null &
fi
echo "Installed $lib/scrawl (on PATH as $bin/scrawl) and started it"
