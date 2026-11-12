#!/usr/bin/env bash
# Builds Mika for Apple Silicon and Intel Macs (one universal app and DMG).
#
# With a "Developer ID Application" certificate in the keychain and Apple ID
# credentials, the app is signed, notarized by Apple and stapled, so it opens
# on any Mac without a Gatekeeper warning. Without them it is ad-hoc signed,
# which only opens on other Macs via System Settings > Privacy & Security >
# Open Anyway.
#
# Credentials come from the environment or from .env.release.local (ignored by
# git):
#   APPLE_ID=you@example.com
#   APPLE_PASSWORD=xxxx-xxxx-xxxx-xxxx   # app-specific password, not your login
#   APPLE_TEAM_ID=ABCDE12345
#
# Usage: npm run release:mac
set -euo pipefail

cd "$(dirname "$0")/.."
if [[ -f .env.release.local ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env.release.local
  set +a
fi

TARGET=universal-apple-darwin
for arch in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target list --installed | grep -qx "$arch" || rustup target add "$arch"
done

# The first Developer ID Application identity in the keychain, if any.
identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application: .*\)"/\1/p' | head -1)
signed=false
if [[ -n "$identity" && -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
  signed=true
  export APPLE_SIGNING_IDENTITY="$identity"
  echo "Signing and notarizing as: $identity"
else
  [[ -z "$identity" ]] && echo "warning: no \"Developer ID Application\" certificate in the keychain."
  [[ -n "$identity" ]] && echo "warning: APPLE_ID, APPLE_PASSWORD or APPLE_TEAM_ID is not set."
  echo "warning: building ad-hoc signed; other Macs will show a Gatekeeper warning."
  # Keep Tauri from trying to notarize with partial credentials.
  unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_SIGNING_IDENTITY
fi

# Tauri signs the app and, when credentials are set, notarizes and staples it.
npm run tauri build -- --target "$TARGET"

bundle="src-tauri/target/$TARGET/release/bundle"
app="$bundle/macos/Mika.app"
dmg=$(ls "$bundle"/dmg/Mika_*_universal.dmg)

if $signed; then
  # The DMG is a separate download, so it gets its own notarization ticket.
  codesign --force --sign "$identity" --timestamp "$dmg"
  xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" --wait
  xcrun stapler staple "$dmg"
  spctl --assess --type execute --verbose=2 "$app"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
fi

echo
echo "Architectures: $(lipo -archs "$app/Contents/MacOS/mika")"
echo "Installer:     $dmg"
