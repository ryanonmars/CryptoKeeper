#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
notarize_script="$script_dir/notarize-artifact.sh"
verify_script="$script_dir/verify-artifacts.sh"
test_root=$(mktemp -d)
trap 'rm -rf "$test_root"' EXIT

bin_dir="$test_root/bin"
artifacts_dir="$test_root/artifacts"
mkdir -p "$bin_dir" "$artifacts_dir"
export TERMKEY_TEST_LOG="$test_root/commands.log"

write_shim() {
  local name=$1
  shift
  printf '%s\n' "$@" > "$bin_dir/$name"
  chmod +x "$bin_dir/$name"
}

write_shim xcrun '#!/usr/bin/env bash
set -euo pipefail
printf "xcrun %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
if [[ "$1" == "notarytool" && "$2" == "submit" ]]; then
  case "${TERMKEY_TEST_NOTARY_STATUS:?}" in
    accepted) printf "%s\\n" "{\"id\":\"submission-123\",\"status\":\"Accepted\"}" ;;
    invalid-status) printf "%s\\n" "{\"id\":\"submission-123\",\"status\":\"Invalid\"}" ;;
    *) echo "unknown simulated notary status" >&2; exit 64 ;;
  esac
elif [[ "$1" == "notarytool" && "$2" == "log" ]]; then
  printf "%s\\n" "notary rejection details"
elif [[ "$1" == "stapler" && ( "$2" == "staple" || "$2" == "validate" ) ]]; then
  printf "%s\\n" "The staple and validate action worked!"
fi'

write_shim codesign '#!/usr/bin/env bash
set -euo pipefail
printf "codesign %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
if [[ "$1" == "-dv" ]]; then
  cat >&2 <<OUTPUT
Executable=/tmp/termkey
Identifier=com.ryanonmars.termkey
Authority=Developer ID Application: TermKey (${TERMKEY_TEST_CODESIGN_TEAM:-TEAM123456})
TeamIdentifier=${TERMKEY_TEST_CODESIGN_TEAM:-TEAM123456}
OUTPUT
  if [[ "${TERMKEY_TEST_RUNTIME:-present}" == "present" ]]; then
    printf "%s\\n" "Runtime Version=14.0.0" >&2
  fi
fi'

write_shim pkgutil '#!/usr/bin/env bash
set -euo pipefail
printf "pkgutil %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
cat <<OUTPUT
Package "TermKey":
   Status: signed by a certificate trusted by macOS
   Signed with a valid Developer ID Installer certificate.
   Certificate Chain:
    1. Developer ID Installer: TermKey (${TERMKEY_TEST_PACKAGE_TEAM:-TEAM123456})
OUTPUT'

write_shim spctl '#!/usr/bin/env bash
set -euo pipefail
printf "spctl %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
printf "%s\\n" "accepted" >&2
printf "%s\\n" "source=Notarized Developer ID" >&2'

write_shim ditto '#!/usr/bin/env bash
set -euo pipefail
printf "ditto %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
if [[ "$1" == "-x" && "$2" == "-k" ]]; then
  destination=${4:?}
  mkdir -p "$destination/browser-extension"
  : > "$destination/termkey"
  : > "$destination/termkey-native-host"
  : > "$destination/browser-extension/manifest.json"
fi'

write_shim lipo '#!/usr/bin/env bash
set -euo pipefail
printf "lipo %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
printf "%s\\n" "${TERMKEY_TEST_ARCH:-arm64}"'

write_shim otool '#!/usr/bin/env bash
set -euo pipefail
printf "otool %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
cat <<OUTPUT
Load command 8
      cmd LC_BUILD_VERSION
  cmdsize 32
    minos ${TERMKEY_TEST_MINOS:-11.0}
OUTPUT'

write_shim unzip '#!/usr/bin/env bash
set -euo pipefail
printf "unzip %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
printf "%s\\n" "${TERMKEY_TEST_ZIP_ENTRIES:?}"'

export PATH="$bin_dir:$PATH"

assert_log_contains() {
  local expected=$1
  if ! rg -F --quiet -- "$expected" "$TERMKEY_TEST_LOG"; then
    echo "expected command log to contain: $expected" >&2
    cat "$TERMKEY_TEST_LOG" >&2
    exit 1
  fi
}

assert_log_not_contains() {
  local unexpected=$1
  if rg -F --quiet -- "$unexpected" "$TERMKEY_TEST_LOG"; then
    echo "command log unexpectedly contained: $unexpected" >&2
    cat "$TERMKEY_TEST_LOG" >&2
    exit 1
  fi
}

assert_text_not_contains() {
  local text=$1
  local unexpected=$2
  if [[ "$text" == *"$unexpected"* ]]; then
    echo "output unexpectedly contained a credential" >&2
    exit 1
  fi
}

run_case() {
  local status=$1
  local kind=$2
  : > "$TERMKEY_TEST_LOG"
  TERMKEY_TEST_NOTARY_STATUS=$status \
  APPLE_NOTARY_KEY_BASE64="${APPLE_NOTARY_KEY_BASE64-}" \
  APPLE_NOTARY_KEY_PATH="$test_root/AuthKey.p8" \
  APPLE_NOTARY_KEY_ID=key-id \
  APPLE_NOTARY_ISSUER_ID=issuer-id \
  RUNNER_TEMP="$test_root" \
    bash "$notarize_script" "$artifacts_dir/TermKey.$kind" "$kind"
}

: > "$artifacts_dir/TermKey.pkg"
: > "$artifacts_dir/TermKey.dmg"
: > "$artifacts_dir/TermKey.zip"
: > "$test_root/AuthKey.p8"
export APPLE_NOTARY_KEY_BASE64=encoded-notary-key
export TERMKEY_TEST_ZIP_ENTRIES=$'termkey\ntermkey-native-host\nbrowser-extension/\nbrowser-extension/manifest.json'

run_case accepted pkg
assert_log_contains 'notarytool submit'
assert_log_contains 'stapler staple'
assert_log_contains 'stapler validate'

run_case accepted dmg
assert_log_contains 'notarytool submit'
assert_log_contains 'stapler staple'
assert_log_contains 'stapler validate'

if rejection_output=$(run_case invalid-status dmg 2>&1); then
  echo 'rejected notarization unexpectedly succeeded' >&2
  exit 1
fi
assert_log_contains 'notarytool log'
if [[ "$rejection_output" != *'log saved to '* ]]; then
  echo 'notarization rejection did not report the saved log path' >&2
  exit 1
fi
assert_text_not_contains "$rejection_output" encoded-notary-key
if [[ ! -f "$test_root/termkey-notary-submission-123.log" ]]; then
  echo 'notarization rejection log was not written' >&2
  exit 1
fi

export notarize_script artifacts_dir test_root TERMKEY_TEST_LOG
export -f run_case
: > "$TERMKEY_TEST_LOG"
if env -u APPLE_NOTARY_KEY_BASE64 bash -c 'run_case accepted zip'; then
  echo 'missing notarization key unexpectedly succeeded' >&2
  exit 1
fi

run_case accepted zip
assert_log_contains 'notarytool submit'
assert_log_not_contains 'stapler '

: > "$TERMKEY_TEST_LOG"
bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" \
  "$artifacts_dir/TermKey.dmg" \
  "$artifacts_dir/TermKey.zip" \
  TEAM123456
assert_log_contains 'pkgutil --check-signature'
assert_log_contains 'xcrun stapler validate'
assert_log_contains 'spctl -a -vv -t install'
assert_log_contains 'codesign --verify --strict --verbose=2'
assert_log_contains 'spctl -a -vv -t open --context context:primary-signature'
assert_log_contains 'ditto -x -k'
assert_log_contains 'unzip -Z1'
assert_log_contains 'lipo -archs'
assert_log_contains 'otool -l'

if TERMKEY_TEST_ARCH=x86_64 bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'non-arm64 ZIP executable unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_MINOS=12.0 bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'wrong deployment target unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_CODESIGN_TEAM=OTHERTEAM bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'wrong code-signing team unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_RUNTIME=missing bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'non-runtime code signature unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_PACKAGE_TEAM=OTHERTEAM bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'wrong installer-signing team unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_ZIP_ENTRIES=$'termkey\ntermkey-native-host\nunexpected-root/' bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'unexpected ZIP root unexpectedly passed verification' >&2
  exit 1
fi

echo 'release tool tests passed'
