#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <pkg-path> <dmg-path> <zip-path> <team-id>" >&2
  exit 1
fi

pkg_path=$1
dmg_path=$2
zip_path=$3
team_id=$4

for artifact_path in "$pkg_path" "$dmg_path" "$zip_path"; do
  if [[ ! -f "$artifact_path" ]]; then
    echo "artifact not found: $artifact_path" >&2
    exit 1
  fi
done

if [[ ! "$team_id" =~ ^[A-Z0-9]{10}$ ]]; then
  echo "team id must be exactly 10 uppercase alphanumeric characters" >&2
  exit 1
fi

require_output_line() {
  local output=$1
  local pattern=$2
  local description=$3

  if ! printf '%s\n' "$output" | grep -Eq -- "$pattern"; then
    echo "$description did not contain a required line: $pattern" >&2
    printf '%s\n' "$output" >&2
    exit 1
  fi
}

verify_macho_target() {
  local executable_path=$1
  local actual_architecture
  local minimum_version

  actual_architecture=$(lipo -archs "$executable_path")
  if [[ "$actual_architecture" != "arm64" ]]; then
    echo "unexpected Mach-O architecture for $executable_path: $actual_architecture" >&2
    exit 1
  fi

  minimum_version=$(otool -l "$executable_path" | awk '
    $1 == "cmd" && $2 == "LC_BUILD_VERSION" {
      command = "build"
      next
    }
    $1 == "cmd" && $2 == "LC_VERSION_MIN_MACOSX" {
      command = "legacy"
      next
    }
    command == "build" && $1 == "minos" {
      print $2
      exit
    }
    command == "legacy" && $1 == "version" {
      print $2
      exit
    }
  ')
  case "$minimum_version" in
    11.0 | 11.0.0) ;;
    *)
      echo "unexpected minimum macOS version for $executable_path: ${minimum_version:-missing}" >&2
      exit 1
      ;;
  esac
}

verify_macho_signature() {
  local executable_path=$1
  local require_runtime=$2
  local signature_details

  codesign --verify --strict --verbose=2 "$executable_path"
  signature_details=$(codesign -dv --verbose=4 "$executable_path" 2>&1)
  require_output_line "$signature_details" "^Authority=Developer ID Application: .* \\($team_id\\)$" "signature for $executable_path"
  require_output_line "$signature_details" "^TeamIdentifier=$team_id$" "signature for $executable_path"
  require_output_line "$signature_details" '^Timestamp=.+$' "signature for $executable_path"

  if [[ "$require_runtime" == "yes" ]]; then
    require_output_line "$signature_details" '^Runtime Version=.+$' "signature for $executable_path"
  fi
}

pkg_signature=$(pkgutil --check-signature "$pkg_path" 2>&1)
require_output_line "$pkg_signature" "^[[:space:]]*[0-9]+\\. Developer ID Installer: .* \\($team_id\\)$" 'package signature'
xcrun stapler validate "$pkg_path"
pkg_assessment=$(spctl -a -vv -t install "$pkg_path" 2>&1)
require_output_line "$pkg_assessment" '^accepted$' 'package Gatekeeper assessment'

verify_macho_signature "$dmg_path" no
xcrun stapler validate "$dmg_path"
dmg_assessment=$(spctl -a -vv -t open --context context:primary-signature "$dmg_path" 2>&1)
require_output_line "$dmg_assessment" '^accepted$' 'disk image Gatekeeper assessment'

zip_entries=$(unzip -Z1 "$zip_path")
zip_roots=''
while IFS= read -r entry || [[ -n "$entry" ]]; do
  if [[ -z "$entry" || "$entry" == /* || "$entry" == *\\* || "$entry" == *'//' ]]; then
    echo "noncanonical ZIP entry: $entry" >&2
    exit 1
  fi

  canonical_entry=$entry
  if [[ "$canonical_entry" == */ ]]; then
    canonical_entry=${canonical_entry%/}
  fi
  if [[ -z "$canonical_entry" ]]; then
    echo "noncanonical ZIP entry: $entry" >&2
    exit 1
  fi

  IFS=/ read -r -a path_components <<< "$canonical_entry"
  for component in "${path_components[@]}"; do
    if [[ -z "$component" || "$component" == . || "$component" == .. ]]; then
      echo "noncanonical ZIP entry: $entry" >&2
      exit 1
    fi
  done
  unset IFS

  zip_roots+="${path_components[0]}"$'\n'
done <<< "$zip_entries"
zip_roots=$(printf '%s' "$zip_roots" | LC_ALL=C sort -u)
expected_zip_roots=$'browser-extension\ntermkey\ntermkey-native-host'
if [[ "$zip_roots" != "$expected_zip_roots" ]]; then
  echo "unexpected ZIP root entries:" >&2
  printf '%s\n' "$zip_roots" >&2
  exit 1
fi

zip_extract_dir=$(mktemp -d)
trap 'rm -rf "$zip_extract_dir"' EXIT
ditto -x -k "$zip_path" "$zip_extract_dir"

termkey_binary="$zip_extract_dir/termkey"
native_host_binary="$zip_extract_dir/termkey-native-host"
if [[ ! -f "$termkey_binary" || ! -f "$native_host_binary" || ! -d "$zip_extract_dir/browser-extension" ]]; then
  echo "ZIP extraction did not contain the expected release payload" >&2
  exit 1
fi

for executable_path in "$termkey_binary" "$native_host_binary"; do
  verify_macho_target "$executable_path"
  verify_macho_signature "$executable_path" yes
done
