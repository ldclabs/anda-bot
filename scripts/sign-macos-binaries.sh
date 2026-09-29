#!/bin/sh
# Sign macOS release binaries with the Developer ID certificate and notarize
# them. Bare executables cannot be stapled; Gatekeeper looks the notarization
# ticket up online.
#
# Usage: scripts/sign-macos-binaries.sh <file>...
#   anda-*          identifier ai.anda.anda-bot, with the anda entitlements
#                   (voice input needs the microphone under the hardened runtime)
#   anda_launcher-* identifier ai.anda.anda-bot.launcher, no entitlements
#
# Required environment (the desktop-release GitHub environment):
#   MAC_CSC_LINK                 base64 of the Developer ID Application .p12
#   MAC_CSC_KEY_PASSWORD         password of that .p12
#   APPLE_ID, APPLE_APP_SPECIFIC_PASSWORD, APPLE_TEAM_ID
#                                account that submits to the notary service

set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
ENTITLEMENTS="${ROOT}/anda_bot/assets/anda.entitlements.plist"
# Issuer of current Developer ID certificates; codesign needs it to embed the
# certificate chain.
DEVELOPER_ID_CA_URL="https://www.apple.com/certificateauthority/DeveloperIDG2CA.cer"

error() { printf 'Error: %s\n' "$1" >&2; exit 1; }

[ "$#" -gt 0 ] || error "Usage: $0 <file>..."
: "${MAC_CSC_LINK:?}" "${MAC_CSC_KEY_PASSWORD:?}"
: "${APPLE_ID:?}" "${APPLE_APP_SPECIFIC_PASSWORD:?}" "${APPLE_TEAM_ID:?}"

WORK=$(mktemp -d)
KEYCHAIN="${WORK}/signing.keychain-db"
KEYCHAIN_PASSWORD=$(openssl rand -hex 24)
USER_KEYCHAINS=$(security list-keychains -d user | tr -d '"')

cleanup() {
    # shellcheck disable=SC2086 # one keychain path per word
    security list-keychains -d user -s $USER_KEYCHAINS
    security delete-keychain "$KEYCHAIN" 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
printf '%s' "$MAC_CSC_LINK" | base64 --decode > "${WORK}/certificate.p12"
security import "${WORK}/certificate.p12" -k "$KEYCHAIN" -P "$MAC_CSC_KEY_PASSWORD" -T /usr/bin/codesign
curl -fsSL "$DEVELOPER_ID_CA_URL" -o "${WORK}/developer-id-ca.cer"
# An exported .p12 may already carry it.
security import "${WORK}/developer-id-ca.cer" -k "$KEYCHAIN" 2>/dev/null || true
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null
# shellcheck disable=SC2086
security list-keychains -d user -s "$KEYCHAIN" $USER_KEYCHAINS

IDENTITY=$(security find-identity -v -p codesigning "$KEYCHAIN" | awk '/"Developer ID Application:/ { print $2; exit }')
[ -n "$IDENTITY" ] || error "MAC_CSC_LINK has no valid Developer ID Application identity"

sign() {
    codesign --force --sign "$IDENTITY" --keychain "$KEYCHAIN" --options runtime --timestamp "$@"
}

mkdir "${WORK}/notarize"
for file in "$@"; do
    case "$(basename "$file")" in
        anda_launcher-*) sign --identifier ai.anda.anda-bot.launcher "$file" ;;
        anda-*) sign --identifier ai.anda.anda-bot --entitlements "$ENTITLEMENTS" "$file" ;;
        *) error "Not an anda release binary: $file" ;;
    esac
    codesign --verify --strict --verbose=2 "$file"
    cp "$file" "${WORK}/notarize/"
done

ditto -c -k "${WORK}/notarize" "${WORK}/notarize.zip"
xcrun notarytool submit "${WORK}/notarize.zip" \
    --apple-id "$APPLE_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID" \
    --wait --timeout 1h --output-format json > "${WORK}/notarization.json" || true
STATUS=$(plutil -extract status raw -o - "${WORK}/notarization.json" 2>/dev/null || true)
if [ "$STATUS" != "Accepted" ]; then
    cat "${WORK}/notarization.json" >&2
    SUBMISSION=$(plutil -extract id raw -o - "${WORK}/notarization.json" 2>/dev/null || true)
    if [ -n "$SUBMISSION" ]; then
        xcrun notarytool log "$SUBMISSION" \
            --apple-id "$APPLE_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID" >&2 || true
    fi
    error "Notarization did not succeed (status: ${STATUS:-unknown})"
fi
printf 'Signed and notarized: %s\n' "$*"
