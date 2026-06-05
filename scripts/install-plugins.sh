#!/usr/bin/env bash
set -euo pipefail

profile="${1:-debug}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_dir="$repo_root/target/$profile"
target_dir="/home/dashie/.config/oxibar/plugins"

plugins=(
  audio
  bluetooth
  clock
  network
  notifications
  tray
  workspaces
)

mkdir -p "$target_dir"

for plugin in "${plugins[@]}"; do
  source="$source_dir/lib${plugin}.so"
  if [[ ! -f "$source" ]]; then
    printf 'missing plugin: %s\n' "$source" >&2
    printf 'build first with: cargo build --workspace\n' >&2
    exit 1
  fi
  install -m 755 "$source" "$target_dir/lib${plugin}.so"
done

printf 'installed %d plugins to %s\n' "${#plugins[@]}" "$target_dir"
