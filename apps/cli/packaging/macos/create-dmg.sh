#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <package-path> <asset-name> <volume-name> <output-dir>" >&2
  exit 1
fi

package_path=$1
asset_name=$2
volume_name=$3
output_dir=$4
icon_path=""

if [[ ! -f "$package_path" ]]; then
  echo "package not found: $package_path" >&2
  exit 1
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
staging_dir=$(mktemp -d)
mount_dir=$(mktemp -d)
tmp_dmg="$output_dir/${asset_name}-temp.dmg"
trap 'rm -rf "$staging_dir" "$mount_dir"; rm -f "$tmp_dmg"' EXIT

mkdir -p "$output_dir"

if [[ -f "$script_dir/termkey.icns" ]]; then
  icon_path="$script_dir/termkey.icns"
fi

cp "$package_path" "$staging_dir/Install TermKey.pkg"
cp "$script_dir/README.txt" "$staging_dir/README.txt"

chmod 644 "$staging_dir/Install TermKey.pkg"
chmod 644 "$staging_dir/README.txt"

run_with_retries() {
  local attempts=$1
  shift

  local try=1
  local exit_code
  while true; do
    if "$@"; then
      return 0
    else
      exit_code=$?
    fi

    if (( try >= attempts )); then
      return "$exit_code"
    fi

    sleep "$try"
    try=$((try + 1))
  done
}

run_with_retries 3 hdiutil create \
  -volname "$volume_name" \
  -srcfolder "$staging_dir" \
  -fs HFS+ \
  -ov \
  -format UDRW \
  "$tmp_dmg" \
  >/dev/null

if [[ -n "$icon_path" ]] && command -v SetFile >/dev/null 2>&1; then
  device=$(run_with_retries 3 hdiutil attach -readwrite -nobrowse -noverify -mountpoint "$mount_dir" "$tmp_dmg" | awk '/Apple_HFS/ {print $1; exit}')
  if [[ -n "$device" ]]; then
    cp "$icon_path" "$mount_dir/.VolumeIcon.icns"
    SetFile -a C "$mount_dir" || true
    SetFile -a V "$mount_dir/.VolumeIcon.icns" || true
    run_with_retries 3 hdiutil detach "$device" >/dev/null || hdiutil detach -force "$device" >/dev/null
  fi
fi

run_with_retries 3 hdiutil convert \
  "$tmp_dmg" \
  -ov \
  -format UDZO \
  -o "$output_dir/${asset_name}.dmg" \
  >/dev/null
