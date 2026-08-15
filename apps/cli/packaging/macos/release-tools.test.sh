#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
notarize_script="$script_dir/notarize-artifact.sh"
verify_script="$script_dir/verify-artifacts.sh"
create_dmg_script="$script_dir/create-dmg.sh"
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
  [[ $# -eq 12 && "$4" == "--key" && "$6" == "--key-id" && "$8" == "--issuer" && "${10}" == "--wait" && "${11}" == "--output-format" && "${12}" == "json" ]] || { echo "unexpected notary submit invocation: $*" >&2; exit 64; }
  case "${TERMKEY_TEST_NOTARY_STATUS:?}" in
    accepted) printf "%s\\n" "{\"id\":\"submission-123\",\"status\":\"Accepted\"}" ;;
    invalid-status) printf "%s\\n" "{\"id\":\"submission-123\",\"status\":\"Invalid\"}" ;;
    *) echo "unknown simulated notary status" >&2; exit 64 ;;
  esac
elif [[ "$1" == "notarytool" && "$2" == "log" ]]; then
  [[ $# -eq 9 && "$3" == "submission-123" && "$4" == "--key" && "$6" == "--key-id" && "$8" == "--issuer" ]] || exit 64
  printf "%s\\n" "notary rejection details"
elif [[ "$1" == "stapler" && ( "$2" == "staple" || "$2" == "validate" ) ]]; then
  [[ $# -eq 3 && ( "$3" == "$TERMKEY_TEST_PKG_PATH" || "$3" == "$TERMKEY_TEST_DMG_PATH" ) ]] || exit 64
  if [[ "${TERMKEY_TEST_FAIL_STAPLER:-}" == "$3" ]]; then
    exit 70
  fi
  printf "%s\\n" "The staple and validate action worked!"
else
  exit 64
fi'

write_shim codesign '#!/usr/bin/env bash
set -euo pipefail
printf "codesign %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
case "$1,${2-},${3-},${4-}" in
  --verify,--strict,--verbose=2,*)
    [[ $# -eq 4 && ( "$4" == "$TERMKEY_TEST_DMG_PATH" || "$4" == */termkey || "$4" == */termkey-native-host ) ]] || exit 64
    [[ "${TERMKEY_TEST_FAIL_CODESIGN_VERIFY:-}" != "$4" ]] || exit 70
    ;;
  -dv,--verbose=4,*,*)
    [[ $# -eq 3 && ( "$3" == "$TERMKEY_TEST_DMG_PATH" || "$3" == */termkey || "$3" == */termkey-native-host ) ]] || exit 64
    ;;
  *) exit 64 ;;
esac
if [[ "$1" == "-dv" ]]; then
  cat >&2 <<OUTPUT
Executable=/tmp/termkey
Identifier=com.ryanonmars.termkey
Authority=Developer ID Application: TermKey (${TERMKEY_TEST_CODESIGN_TEAM:-TEAM123456})
TeamIdentifier=${TERMKEY_TEST_CODESIGN_TEAM:-TEAM123456}
Timestamp=${TERMKEY_TEST_CODESIGN_TIMESTAMP-2026-08-14T12:00:00Z}
OUTPUT
  if [[ "${TERMKEY_TEST_RUNTIME:-present}" == "present" ]]; then
    printf "%s\\n" "Runtime Version=14.0.0" >&2
  fi
fi'

write_shim pkgutil '#!/usr/bin/env bash
set -euo pipefail
printf "pkgutil %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
[[ $# -eq 2 && "$1" == "--check-signature" && "$2" == "$TERMKEY_TEST_PKG_PATH" ]] || exit 64
[[ "${TERMKEY_TEST_FAIL_PKGUTIL:-}" != "1" ]] || exit 70
cat <<OUTPUT
Package "TermKey":
   Status: signed by a certificate trusted by macOS ${TERMKEY_TEST_PACKAGE_DECOY_TEAM-}
    Signed with a valid Developer ID Installer certificate.
   Certificate Chain:
    1. Developer ID Installer: TermKey (${TERMKEY_TEST_PACKAGE_TEAM:-TEAM123456})
OUTPUT'

write_shim spctl '#!/usr/bin/env bash
set -euo pipefail
printf "spctl %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
[[ "$1" == "-a" && "$2" == "-vv" && "$3" == "-t" ]] || exit 64
if [[ "$4" == "install" ]]; then
  [[ $# -eq 5 && "$5" == "$TERMKEY_TEST_PKG_PATH" ]] || exit 64
  assessment_path=$5
elif [[ "$4" == "open" ]]; then
  [[ $# -eq 7 && "$5" == "--context" && "$6" == "context:primary-signature" && "$7" == "$TERMKEY_TEST_DMG_PATH" ]] || exit 64
  assessment_path=$7
else
  exit 64
fi
[[ "${TERMKEY_TEST_FAIL_SPCTL:-}" != "$4" ]] || exit 70
printf "%s: accepted\\n" "${TERMKEY_TEST_SPCTL_ACCEPTED_PATH:-$assessment_path}" >&2
printf "%s\\n" "source=Notarized Developer ID" >&2'

write_shim hdiutil '#!/usr/bin/env bash
set -euo pipefail
printf "hdiutil %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
case "$1" in
  create)
    if [[ "${TERMKEY_TEST_HDIUTIL_FAIL:-}" == "create" ]]; then
      exit 71
    fi
    output_path=${!#}
    : > "$output_path"
    ;;
  attach)
    [[ "${TERMKEY_TEST_HDIUTIL_FAIL:-}" != "attach" ]] || exit 72
    printf "%s\\n" "/dev/disk99 Apple_HFS"
    ;;
  detach)
    if [[ "${2-}" != "-force" && "${TERMKEY_TEST_HDIUTIL_FAIL:-}" == "detach" ]]; then
      exit 73
    fi
    ;;
  convert)
    if [[ "${TERMKEY_TEST_HDIUTIL_FAIL:-}" == "convert" ]]; then
      exit 74
    fi
    while (( $# > 0 )); do
      if [[ "$1" == "-o" ]]; then
        : > "$2"
        exit 0
      fi
      shift
    done
    exit 64
    ;;
  *) exit 64 ;;
esac'

write_shim SetFile '#!/usr/bin/env bash
set -euo pipefail
printf "SetFile %s\\n" "$*" >> "$TERMKEY_TEST_LOG"'

write_shim sleep '#!/usr/bin/env bash
set -euo pipefail
printf "sleep %s\\n" "$*" >> "$TERMKEY_TEST_LOG"'

write_shim ditto '#!/usr/bin/env bash
set -euo pipefail
printf "ditto %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
if [[ "$1" == "-x" && "$2" == "-k" && "$3" == "$TERMKEY_TEST_ZIP_PATH" && $# -eq 4 ]]; then
  destination=${4:?}
  mkdir -p "$destination/browser-extension"
  : > "$destination/termkey"
  : > "$destination/termkey-native-host"
  : > "$destination/browser-extension/manifest.json"
else
  exit 64
fi'

write_shim lipo '#!/usr/bin/env bash
set -euo pipefail
printf "lipo %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
[[ $# -eq 2 && "$1" == "-archs" && ( "$2" == */termkey || "$2" == */termkey-native-host ) ]] || exit 64
printf "%s\\n" "${TERMKEY_TEST_ARCH:-arm64}"'

write_shim otool '#!/usr/bin/env bash
set -euo pipefail
printf "otool %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
[[ $# -eq 2 && "$1" == "-l" && ( "$2" == */termkey || "$2" == */termkey-native-host ) ]] || exit 64
cat <<OUTPUT
Load command 8
      cmd LC_BUILD_VERSION
  cmdsize 32
    minos ${TERMKEY_TEST_MINOS:-11.0}
OUTPUT'

write_shim unzip '#!/usr/bin/env bash
set -euo pipefail
printf "unzip %s\\n" "$*" >> "$TERMKEY_TEST_LOG"
[[ $# -eq 2 && "$1" == "-Z1" && "$2" == "$TERMKEY_TEST_ZIP_PATH" ]] || exit 64
printf "%s\\n" "${TERMKEY_TEST_ZIP_ENTRIES:?}"'

export PATH="$bin_dir:$PATH"

assert_log_contains() {
  local expected=$1
  if ! grep -Fq -- "$expected" "$TERMKEY_TEST_LOG"; then
    echo "expected command log to contain: $expected" >&2
    cat "$TERMKEY_TEST_LOG" >&2
    exit 1
  fi
}

assert_log_not_contains() {
  local unexpected=$1
  if grep -Fq -- "$unexpected" "$TERMKEY_TEST_LOG"; then
    echo "command log unexpectedly contained: $unexpected" >&2
    cat "$TERMKEY_TEST_LOG" >&2
    exit 1
  fi
}

assert_log_count() {
  local expected=$1
  local count=$2
  local actual
  actual=$(grep -Fc -- "$expected" "$TERMKEY_TEST_LOG" || true)
  if [[ "$actual" != "$count" ]]; then
    echo "expected $count occurrences of: $expected (found $actual)" >&2
    cat "$TERMKEY_TEST_LOG" >&2
    exit 1
  fi
}

assert_log_regex_count() {
  local pattern=$1
  local count=$2
  local actual
  actual=$(grep -Ec -- "$pattern" "$TERMKEY_TEST_LOG" || true)
  if [[ "$actual" != "$count" ]]; then
    echo "expected $count matching commands: $pattern (found $actual)" >&2
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
export TERMKEY_TEST_PKG_PATH="$artifacts_dir/TermKey.pkg"
export TERMKEY_TEST_DMG_PATH="$artifacts_dir/TermKey.dmg"
export TERMKEY_TEST_ZIP_PATH="$artifacts_dir/TermKey.zip"
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
assert_log_count "pkgutil --check-signature $artifacts_dir/TermKey.pkg" 1
assert_log_count "xcrun stapler validate $artifacts_dir/TermKey.pkg" 1
assert_log_count "xcrun stapler validate $artifacts_dir/TermKey.dmg" 1
assert_log_count "spctl -a -vv -t install $artifacts_dir/TermKey.pkg" 1
assert_log_count "codesign --verify --strict --verbose=2 $artifacts_dir/TermKey.dmg" 1
assert_log_count "codesign -dv --verbose=4 $artifacts_dir/TermKey.dmg" 1
assert_log_count "spctl -a -vv -t open --context context:primary-signature $artifacts_dir/TermKey.dmg" 1
assert_log_count "ditto -x -k $artifacts_dir/TermKey.zip" 1
assert_log_count "unzip -Z1 $artifacts_dir/TermKey.zip" 1
assert_log_regex_count '^codesign --verify --strict --verbose=2 .*/termkey$' 1
assert_log_regex_count '^codesign --verify --strict --verbose=2 .*/termkey-native-host$' 1
assert_log_regex_count '^codesign -dv --verbose=4 .*/termkey$' 1
assert_log_regex_count '^codesign -dv --verbose=4 .*/termkey-native-host$' 1
assert_log_regex_count '^lipo -archs .*/termkey$' 1
assert_log_regex_count '^lipo -archs .*/termkey-native-host$' 1
assert_log_regex_count '^otool -l .*/termkey$' 1
assert_log_regex_count '^otool -l .*/termkey-native-host$' 1

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

if TERMKEY_TEST_PACKAGE_TEAM=OTHERTEAM TERMKEY_TEST_PACKAGE_DECOY_TEAM=TEAM123456 bash "$verify_script" \
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

if TERMKEY_TEST_CODESIGN_TIMESTAMP='' bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'DMG without a signing timestamp unexpectedly passed verification' >&2
  exit 1
fi

if bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" INVALID \
  >/dev/null 2>&1; then
  echo 'malformed Team ID unexpectedly passed verification' >&2
  exit 1
fi

for malformed_entries in \
  $'/termkey\ntermkey-native-host\nbrowser-extension/' \
  $'termkey/../escape\ntermkey-native-host\nbrowser-extension/' \
  $'termkey//extra\ntermkey-native-host\nbrowser-extension/' \
  $'termkey\\backslash\ntermkey-native-host\nbrowser-extension/' \
  $'./termkey\ntermkey-native-host\nbrowser-extension/'; do
  : > "$TERMKEY_TEST_LOG"
  if TERMKEY_TEST_ZIP_ENTRIES="$malformed_entries" bash "$verify_script" \
    "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
    >/dev/null 2>&1; then
    echo 'noncanonical ZIP entry unexpectedly passed verification' >&2
    exit 1
  fi
  assert_log_not_contains 'ditto -x -k'
done

if TERMKEY_TEST_FAIL_PKGUTIL=1 bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'pkgutil failure unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_FAIL_STAPLER="$artifacts_dir/TermKey.pkg" bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'stapler failure unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_FAIL_SPCTL=install bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'Gatekeeper failure unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_SPCTL_ACCEPTED_PATH="$artifacts_dir/decoy.pkg" bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'Gatekeeper acceptance for a different artifact unexpectedly passed verification' >&2
  exit 1
fi

if TERMKEY_TEST_FAIL_CODESIGN_VERIFY="$artifacts_dir/TermKey.dmg" bash "$verify_script" \
  "$artifacts_dir/TermKey.pkg" "$artifacts_dir/TermKey.dmg" "$artifacts_dir/TermKey.zip" TEAM123456 \
  >/dev/null 2>&1; then
  echo 'codesign verification failure unexpectedly passed verification' >&2
  exit 1
fi

dmg_test_failures=0

: > "$TERMKEY_TEST_LOG"
create_failure_output="$test_root/dmg-create-failure"
mkdir -p "$create_failure_output"
if TERMKEY_TEST_HDIUTIL_FAIL=create bash "$create_dmg_script" \
  "$artifacts_dir/TermKey.pkg" termkey-create-failure 'TermKey Test' "$create_failure_output"; then
  echo 'exhausted hdiutil create failures unexpectedly succeeded' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi
if [[ $(grep -Fc -- 'hdiutil create ' "$TERMKEY_TEST_LOG" || true) != 3 ]]; then
  echo 'hdiutil create was not attempted exactly three times' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi
if grep -Fq -- 'hdiutil convert ' "$TERMKEY_TEST_LOG"; then
  echo 'DMG conversion ran after exhausted hdiutil create failures' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi

: > "$TERMKEY_TEST_LOG"
convert_failure_output="$test_root/dmg-convert-failure"
mkdir -p "$convert_failure_output"
if TERMKEY_TEST_HDIUTIL_FAIL=convert bash "$create_dmg_script" \
  "$artifacts_dir/TermKey.pkg" termkey-convert-failure 'TermKey Test' "$convert_failure_output"; then
  echo 'exhausted hdiutil convert failures unexpectedly succeeded' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi
if [[ $(grep -Fc -- 'hdiutil convert ' "$TERMKEY_TEST_LOG" || true) != 3 ]]; then
  echo 'hdiutil convert was not attempted exactly three times' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi

: > "$TERMKEY_TEST_LOG"
detach_failure_output="$test_root/dmg-detach-failure"
mkdir -p "$detach_failure_output"
TERMKEY_TEST_HDIUTIL_FAIL=detach bash "$create_dmg_script" \
  "$artifacts_dir/TermKey.pkg" termkey-detach-failure 'TermKey Test' "$detach_failure_output"
if [[ $(grep -Fc -- 'hdiutil detach /dev/disk99' "$TERMKEY_TEST_LOG" || true) != 3 ]]; then
  echo 'hdiutil detach was not attempted exactly three times before forcing' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi
if [[ $(grep -Fc -- 'hdiutil detach -force /dev/disk99' "$TERMKEY_TEST_LOG" || true) != 1 ]]; then
  echo 'hdiutil detach did not force-detach after exhausted normal attempts' >&2
  dmg_test_failures=$((dmg_test_failures + 1))
fi

if (( dmg_test_failures > 0 )); then
  exit 1
fi

echo 'release tool tests passed'
