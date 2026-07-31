#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/.." && pwd)
source_svg="$repo_root/assets/branding/termkey-icon.svg"
mac_icon="$repo_root/packaging/macos/termkey.icns"

for tool in rsvg-convert magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing required tool: $tool" >&2
    exit 1
  fi
done

if [[ ! -f "$source_svg" ]]; then
  echo "missing source SVG: $source_svg" >&2
  exit 1
fi

tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

logo_png="$tmp_dir/termkey-logo.png"
fitted_png="$tmp_dir/termkey-fitted.png"
mark_png="$tmp_dir/termkey-mark.png"

mkdir -p "$(dirname "$mac_icon")"

# Fit the full SVG inside a square icon canvas with padding so installer icons
# show the entire artwork instead of clipping it.
rsvg-convert -w 1024 "$source_svg" -o "$logo_png"
magick "$logo_png" -background none -gravity center -resize 920x920 "$fitted_png"
magick "$fitted_png" -background none -gravity center -extent 1024x1024 "$mark_png"

declare -a icon_sizes=(16 32 64 128 256 512 1024)
declare -a icon_types=(icp4 icp5 icp6 ic07 ic08 ic09 ic10)
png_dir="$tmp_dir/icns-png"
mkdir -p "$png_dir"

for size in "${icon_sizes[@]}"; do
  magick "$mark_png" -resize "${size}x${size}" "PNG32:$png_dir/${size}.png"
done

python3 - "$png_dir" "$mac_icon" <<'PY'
import os
import struct
import sys

png_dir = sys.argv[1]
output_path = sys.argv[2]
mapping = [
    ("icp4", "16.png"),
    ("icp5", "32.png"),
    ("icp6", "64.png"),
    ("ic07", "128.png"),
    ("ic08", "256.png"),
    ("ic09", "512.png"),
    ("ic10", "1024.png"),
]

chunks = []
for ostype, filename in mapping:
    path = os.path.join(png_dir, filename)
    with open(path, "rb") as f:
        data = f.read()
    chunks.append(ostype.encode("ascii") + struct.pack(">I", len(data) + 8) + data)

body = b"".join(chunks)
with open(output_path, "wb") as f:
    f.write(b"icns")
    f.write(struct.pack(">I", len(body) + 8))
    f.write(body)
PY
