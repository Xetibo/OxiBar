#!/usr/bin/env bash
set -euo pipefail

profile="${1:-debug}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
[[ "$build_dir" = /* ]] || build_dir="$repo_root/$build_dir"
source_dir="$build_dir/$profile"
target_dir="${XDG_CONFIG_HOME:-$HOME/.config}/oxibar/plugins"

plugins=(
  audio
  applications
  battery
  bluetooth
  clock
  network
  notifications
  tray
  workspaces
  stats
  system
  tools
)

for plugin in "${plugins[@]}"; do
  source="$source_dir/lib${plugin}.so"
  if [[ ! -f "$source" ]]; then
    printf 'missing plugin: %s\n' "$source" >&2
    printf 'build first with: cargo build --workspace\n' >&2
    exit 1
  fi
done

mkdir -p "$target_dir"
for plugin in "${plugins[@]}"; do
  install -m 755 "$source_dir/lib${plugin}.so" "$target_dir/lib${plugin}.so"
done

printf 'installed %d plugins to %s\n' "${#plugins[@]}" "$target_dir"
