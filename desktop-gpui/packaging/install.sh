#!/bin/sh
# Installs the interval-desktop binary plus its Linux desktop entry and icons for the
# current user. Run from the repo root after `cargo build --release -p interval-desktop`.
set -eu

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
binary="$repo_root/target/release/interval-desktop"
[ -x "$binary" ] || { echo "build first: cargo build --release -p interval-desktop" >&2; exit 1; }

mkdir -p "$HOME/.local/bin"
install -m 755 "$binary" "$HOME/.local/bin/interval-desktop"

apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$apps_dir"
# Exec must be an absolute path: ~/.local/bin is not on PATH for every session type.
sed "s|^Exec=.*|Exec=$HOME/.local/bin/interval-desktop|" \
  "$repo_root/desktop-gpui/packaging/interval.desktop" > "$apps_dir/interval.desktop"

for size in 32 64 128 256; do
  icon_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$icon_dir"
  cp "$repo_root/desktop-gpui/app-icon/icon-$size.png" "$icon_dir/interval.png"
done

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" || true

echo "installed: $HOME/.local/bin/interval-desktop and $apps_dir/interval.desktop"
