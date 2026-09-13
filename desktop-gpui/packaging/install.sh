#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
binary="${1:-$repo_root/target/release/interval-desktop}"
install_dir="${2:-$HOME/.local/bin}"
[ -x "$binary" ] || { echo "build first: cargo build --release -p interval-desktop" >&2; exit 1; }

mkdir -p "$install_dir"
install_dir="$(cd "$install_dir" && pwd)"
install -m 755 "$binary" "$install_dir/interval-desktop"

apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$apps_dir"
exec_path="$install_dir/interval-desktop"
exec_path="${exec_path//\\/\\\\}"
exec_path="${exec_path//\"/\\\"}"
exec_path="${exec_path//\$/\\\$}"
exec_path="${exec_path//\`/\\\`}"
exec_path="${exec_path//\\/\\\\}"
exec_path="${exec_path//%/%%}"
exec_path="${exec_path//$'\n'/\\n}"
exec_path="${exec_path//$'\r'/\\r}"
exec_path="${exec_path//$'\t'/\\t}"
while IFS= read -r line; do
  case "$line" in
    Exec=*) printf 'Exec="%s"\n' "$exec_path" ;;
    *) printf '%s\n' "$line" ;;
  esac
done < "$repo_root/desktop-gpui/packaging/interval.desktop" > "$apps_dir/interval.desktop"

for size in 32 64 128 256; do
  icon_dir="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$icon_dir"
  cp "$repo_root/desktop-gpui/app-icon/icon-$size.png" "$icon_dir/interval.png"
done

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" || true

echo "installed: $install_dir/interval-desktop and $apps_dir/interval.desktop"
