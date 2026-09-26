#!/bin/sh
# Build Linen for macOS as a DMG (D3), signed and notarised when the credentials are
# there (pending approval 28), then check the result.
#
# Usage:
#   scripts/release-macos.sh             signed and notarised; stops if credentials are missing
#   scripts/release-macos.sh --unsigned  a development DMG (ad-hoc signed), for local testing
#
# Signing, from the keychain:
#   APPLE_SIGNING_IDENTITY   "Developer ID Application: Name (TEAMID)"
# Notarisation, either an App Store Connect API key:
#   APPLE_API_KEY            key id
#   APPLE_API_ISSUER         issuer id
#   APPLE_API_KEY_PATH       path to AuthKey_<id>.p8 (kept out of the repo)
# or an Apple ID with an app-specific password:
#   APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID
#
# Tauri signs with the hardened runtime and submits to notarytool itself when these
# are set; this script checks them first and verifies the output afterwards.
set -eu
cd "$(dirname "$0")/.."

UNSIGNED=0
[ "${1:-}" = "--unsigned" ] && UNSIGNED=1

fail() { echo "release-macos: $*" >&2; exit 1; }

if [ "$UNSIGNED" = 0 ]; then
  [ -n "${APPLE_SIGNING_IDENTITY:-}" ] || fail "APPLE_SIGNING_IDENTITY is not set (or use --unsigned)"
  security find-identity -v -p codesigning | grep -F "$APPLE_SIGNING_IDENTITY" >/dev/null ||
    fail "no signing identity \"$APPLE_SIGNING_IDENTITY\" in the keychain"
  if [ -n "${APPLE_API_KEY:-}" ]; then
    [ -n "${APPLE_API_ISSUER:-}" ] && [ -f "${APPLE_API_KEY_PATH:-/nonexistent}" ] ||
      fail "APPLE_API_KEY needs APPLE_API_ISSUER and a readable APPLE_API_KEY_PATH"
  elif [ -n "${APPLE_ID:-}" ]; then
    [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] ||
      fail "APPLE_ID needs APPLE_PASSWORD and APPLE_TEAM_ID"
  else
    fail "no notarisation credentials (APPLE_API_KEY… or APPLE_ID…)"
  fi
else
  # Ad-hoc: runs on this Mac; Gatekeeper refuses it elsewhere.
  export APPLE_SIGNING_IDENTITY="-"
  unset APPLE_API_KEY APPLE_ID 2>/dev/null || true
fi

pnpm install --frozen-lockfile
pnpm tauri build --bundles app,dmg

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
BUNDLE=src-tauri/target/release/bundle
APP="$BUNDLE/macos/Linen.app"
DMG=$(ls "$BUNDLE"/dmg/Linen_"$VERSION"_*.dmg | head -1)
[ -d "$APP" ] || fail "no $APP"
[ -f "$DMG" ] || fail "no DMG for $VERSION"

echo "== Checks"
PLIST="$APP/Contents/Info.plist"
[ "$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$PLIST")" = "13.0" ] ||
  fail "LSMinimumSystemVersion is not 13.0 (D7)"
/usr/libexec/PlistBuddy -c 'Print :CFBundleDocumentTypes:0:CFBundleTypeExtensions' "$PLIST" |
  grep -q epub || fail "the .epub file association is missing (Open With, Dock drop)"
codesign --verify --deep --strict --verbose=2 "$APP"
if [ "$UNSIGNED" = 0 ]; then
  codesign -dvv "$APP" 2>&1 | grep -q "flags=.*runtime" || fail "not signed with the hardened runtime"
  xcrun stapler validate "$APP"
  xcrun stapler validate "$DMG"
  spctl --assess --type execute --verbose=2 "$APP"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
fi

shasum -a 256 "$DMG"
echo "== Built $DMG ($([ "$UNSIGNED" = 1 ] && echo 'unsigned development build' || echo 'signed and notarised'))"
