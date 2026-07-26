#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <binary-path> <version> <package-name> <output-dir>" >&2
  exit 1
fi

binary_path=$1
version=$2
package_name=$3
output_dir=$4

if [[ ! -f "$binary_path" ]]; then
  echo "binary not found: $binary_path" >&2
  exit 1
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../../.." && pwd)
staging_dir=$(mktemp -d)
trap 'rm -rf "$staging_dir"' EXIT

payload_root="$staging_dir/root"
cli_install_dir="$payload_root/usr/local/bin"
termkey_app_bundle_dir="$payload_root/Applications/TermKey.app"
uninstall_app_bundle_dir="$payload_root/Applications/Uninstall TermKey.app"
volume_icon="$script_dir/termkey.icns"
plist_template="$script_dir/Info.plist.template"
uninstaller_plist_template="$script_dir/UninstallerInfo.plist.template"
launcher_source="$script_dir/Launcher.swift"
uninstaller_source="$script_dir/Uninstaller.swift"
swift_cache_dir="$staging_dir/swift-cache"
native_host_binary_path=${TERMKEY_NATIVE_HOST_BINARY:-"$(cd "$(dirname "$binary_path")" && pwd)/termkey-native-host"}
extension_source_dir=${TERMKEY_EXTENSION_DIR:-"$repo_root/apps/extension"}
apple_signing_enabled=${APPLE_SIGNING_ENABLED:-false}
: "${TERMKEY_MACOS_ARCH:?TERMKEY_MACOS_ARCH must be x86_64 or arm64}"
: "${MACOSX_DEPLOYMENT_TARGET:?MACOSX_DEPLOYMENT_TARGET is required}"

case "$TERMKEY_MACOS_ARCH" in
  x86_64 | arm64) ;;
  *)
    echo "unsupported macOS architecture: $TERMKEY_MACOS_ARCH" >&2
    exit 1
    ;;
esac

if [[ ! "$MACOSX_DEPLOYMENT_TARGET" =~ ^[0-9]+([.][0-9]+){1,2}$ ]]; then
  echo "invalid macOS deployment target: $MACOSX_DEPLOYMENT_TARGET" >&2
  exit 1
fi

if [[ ! -f "$native_host_binary_path" ]]; then
  echo "native host binary not found: $native_host_binary_path" >&2
  exit 1
fi

if [[ ! -f "$extension_source_dir/manifest.json" || ! -f "$extension_source_dir/popup.html" || ! -f "$extension_source_dir/dist/background.js" ]]; then
  echo "Chrome extension bundle not found or incomplete at: $extension_source_dir" >&2
  echo "build it first with: npm run build:extension" >&2
  exit 1
fi

create_app_bundle() {
  local app_bundle_dir=$1
  local executable_name=$2
  local source_path=$3
  local plist_template_path=$4
  local bundle_cli_binary=$5

  local app_contents_dir="$app_bundle_dir/Contents"
  local app_macos_dir="$app_contents_dir/MacOS"
  local app_resources_dir="$app_contents_dir/Resources"
  local app_info_plist="$app_contents_dir/Info.plist"
  local app_pkg_info="$app_contents_dir/PkgInfo"
  local app_executable="$app_macos_dir/$executable_name"

  mkdir -p "$app_macos_dir" "$app_resources_dir"

  CLANG_MODULE_CACHE_PATH="$swift_cache_dir" \
  SWIFT_MODULE_CACHE_PATH="$swift_cache_dir" \
  swiftc -O -target "${TERMKEY_MACOS_ARCH}-apple-macosx${MACOSX_DEPLOYMENT_TARGET}" \
    -o "$app_executable" \
    "$source_path"
  chmod 755 "$app_executable"

  if [[ "$bundle_cli_binary" == "yes" ]]; then
    local app_binary_dir="$app_resources_dir/bin"
    local app_extension_dir="$app_resources_dir/browser-extension/chrome"
    mkdir -p "$app_binary_dir"
    ditto --noextattr --noqtn "$binary_path" "$app_binary_dir/termkey"
    ditto --noextattr --noqtn "$native_host_binary_path" "$app_binary_dir/termkey-native-host"
    mkdir -p "$app_extension_dir"
    ditto --noextattr --noqtn "$extension_source_dir" "$app_extension_dir"
    chmod 755 "$app_binary_dir/termkey"
    chmod 755 "$app_binary_dir/termkey-native-host"
  fi

  sed "s/__VERSION__/$version/g" "$plist_template_path" > "$app_info_plist"
  printf 'APPL????' > "$app_pkg_info"

  if [[ -f "$volume_icon" ]]; then
    ditto --noextattr --noqtn "$volume_icon" "$app_resources_dir/termkey.icns"
  fi
}

verify_macho_target() {
  local executable_path=$1
  local actual_architecture
  local minimum_version

  actual_architecture=$(lipo -archs "$executable_path")
  if [[ "$actual_architecture" != "$TERMKEY_MACOS_ARCH" ]]; then
    echo "unexpected Mach-O architecture for $executable_path: $actual_architecture" >&2
    exit 1
  fi

  minimum_version=$(
    otool -l "$executable_path" | awk '
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
    '
  )
  case "$minimum_version" in
    "$MACOSX_DEPLOYMENT_TARGET" | "$MACOSX_DEPLOYMENT_TARGET.0") ;;
    *)
      echo "unexpected minimum macOS version for $executable_path: ${minimum_version:-missing}" >&2
      exit 1
      ;;
  esac
}

mkdir -p "$cli_install_dir" "$output_dir"
mkdir -p "$swift_cache_dir"

ditto --noextattr --noqtn "$binary_path" "$cli_install_dir/termkey"
ditto --noextattr --noqtn "$native_host_binary_path" "$cli_install_dir/termkey-native-host"
chmod 755 "$cli_install_dir/termkey"
chmod 755 "$cli_install_dir/termkey-native-host"
create_app_bundle "$termkey_app_bundle_dir" "TermKey" "$launcher_source" "$plist_template" yes
create_app_bundle "$uninstall_app_bundle_dir" "UninstallTermKey" "$uninstaller_source" "$uninstaller_plist_template" no

verify_macho_target "$cli_install_dir/termkey"
verify_macho_target "$cli_install_dir/termkey-native-host"
verify_macho_target "$termkey_app_bundle_dir/Contents/Resources/bin/termkey"
verify_macho_target "$termkey_app_bundle_dir/Contents/Resources/bin/termkey-native-host"
verify_macho_target "$termkey_app_bundle_dir/Contents/MacOS/TermKey"
verify_macho_target "$uninstall_app_bundle_dir/Contents/MacOS/UninstallTermKey"

xattr -cr "$payload_root" 2>/dev/null || true
find "$payload_root" -name '._*' -delete 2>/dev/null || true
dot_clean -m "$payload_root" 2>/dev/null || true

if [[ "$apple_signing_enabled" == "true" ]]; then
  : "${APPLE_APPLICATION_SIGNING_IDENTITY:?APPLE_APPLICATION_SIGNING_IDENTITY is required when Apple signing is enabled}"
  : "${APPLE_INSTALLER_SIGNING_IDENTITY:?APPLE_INSTALLER_SIGNING_IDENTITY is required when Apple signing is enabled}"

  sign_application_item() {
    local item_path=$1
    codesign --force --options runtime --timestamp \
      --sign "$APPLE_APPLICATION_SIGNING_IDENTITY" \
      "$item_path"
    codesign --verify --strict --verbose=2 "$item_path"
  }

  sign_application_item "$cli_install_dir/termkey"
  sign_application_item "$cli_install_dir/termkey-native-host"
  sign_application_item "$termkey_app_bundle_dir/Contents/Resources/bin/termkey"
  sign_application_item "$termkey_app_bundle_dir/Contents/Resources/bin/termkey-native-host"
  sign_application_item "$termkey_app_bundle_dir/Contents/MacOS/TermKey"
  sign_application_item "$termkey_app_bundle_dir"
  sign_application_item "$uninstall_app_bundle_dir/Contents/MacOS/UninstallTermKey"
  sign_application_item "$uninstall_app_bundle_dir"
fi

package_output="$output_dir/${package_name}.pkg"
package_build_output=$package_output
if [[ "$apple_signing_enabled" == "true" ]]; then
  package_build_output="$staging_dir/${package_name}-unsigned.pkg"
fi

COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 pkgbuild \
  --root "$payload_root" \
  --identifier "com.ryanonmars.termkey" \
  --version "$version" \
  --install-location "/" \
  --scripts "$script_dir/scripts" \
  "$package_build_output" \
  >/dev/null

if [[ "$apple_signing_enabled" == "true" ]]; then
  productsign --sign "$APPLE_INSTALLER_SIGNING_IDENTITY" --timestamp \
    "$package_build_output" \
    "$package_output" \
    >/dev/null
  pkgutil --check-signature "$package_output"
fi
