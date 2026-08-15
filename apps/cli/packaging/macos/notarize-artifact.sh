#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <artifact-path> <pkg|dmg|zip>" >&2
  exit 1
fi

artifact_path=$1
artifact_kind=$2

if [[ ! -f "$artifact_path" ]]; then
  echo "artifact not found: $artifact_path" >&2
  exit 1
fi

case "$artifact_kind" in
  pkg | dmg | zip) ;;
  *)
    echo "unsupported artifact kind: $artifact_kind" >&2
    exit 1
    ;;
esac

: "${APPLE_NOTARY_KEY_BASE64:?APPLE_NOTARY_KEY_BASE64 is required}"
: "${APPLE_NOTARY_KEY_PATH:?APPLE_NOTARY_KEY_PATH is required}"
: "${APPLE_NOTARY_KEY_ID:?APPLE_NOTARY_KEY_ID is required}"
: "${APPLE_NOTARY_ISSUER_ID:?APPLE_NOTARY_ISSUER_ID is required}"

if [[ ! -f "$APPLE_NOTARY_KEY_PATH" ]]; then
  echo "notary API key file not found: $APPLE_NOTARY_KEY_PATH" >&2
  exit 1
fi

submission_json=$(xcrun notarytool submit "$artifact_path" \
  --key "$APPLE_NOTARY_KEY_PATH" \
  --key-id "$APPLE_NOTARY_KEY_ID" \
  --issuer "$APPLE_NOTARY_ISSUER_ID" \
  --wait --output-format json)
submission_id=$(node -e 'const v=JSON.parse(process.argv[1]); process.stdout.write(v.id ?? "")' "$submission_json")
status=$(node -e 'const v=JSON.parse(process.argv[1]); process.stdout.write(v.status ?? "")' "$submission_json")

if [[ -z "$submission_id" ]]; then
  echo "notary submission did not return an id" >&2
  exit 1
fi

if [[ "$status" != "Accepted" ]]; then
  : "${RUNNER_TEMP:?RUNNER_TEMP is required to save the notarization log}"
  mkdir -p "$RUNNER_TEMP"
  rejection_log="$RUNNER_TEMP/termkey-notary-$submission_id.log"
  xcrun notarytool log "$submission_id" \
    --key "$APPLE_NOTARY_KEY_PATH" \
    --key-id "$APPLE_NOTARY_KEY_ID" \
    --issuer "$APPLE_NOTARY_ISSUER_ID" \
    > "$rejection_log"
  echo "notarization status was $status; log saved to $rejection_log" >&2
  exit 1
fi

case "$artifact_kind" in
  pkg | dmg)
    xcrun stapler staple "$artifact_path"
    xcrun stapler validate "$artifact_path"
    ;;
  zip)
    ;;
esac
