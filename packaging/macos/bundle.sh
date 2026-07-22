#!/usr/bin/env bash
#
# Build, bundle, sign, notarize and staple the macOS app.
#
#   ./packaging/macos/bundle.sh
#
# Signing and notarization are opt-in via environment variables. Without them
# you still get a working .app and .dmg — just an unsigned one that Gatekeeper
# will complain about, which is fine for local testing.
#
#   SIGN_IDENTITY       e.g. "Developer ID Application: Jane Doe (TEAMID1234)"
#                       Find yours with: security find-identity -v -p codesigning
#
# Then EITHER a stored notarytool profile:
#   NOTARY_PROFILE      created once with:
#                         xcrun notarytool store-credentials <name> \
#                           --apple-id <id> --team-id <team> --password <app-specific-pw>
#
# OR the three values directly (this is what CI uses):
#   APPLE_ID  APPLE_TEAM_ID  APPLE_APP_PASSWORD

set -euo pipefail

cd "$(dirname "$0")/../.."

APP_NAME="Discord Desktop Fixer"
BIN_NAME="discord-desktop-fixer"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)"
DIST="target/dist"
APP="$DIST/$APP_NAME.app"
# DMG is named once the built architectures are known.

say() { printf '\033[1;35m==>\033[0m %s\n' "$1"; }

# ---------------------------------------------------------------- build
# Universal, so a single download works on both Apple Silicon and Intel.
say "Building ($VERSION)"
slices=()
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target add "$target" >/dev/null 2>&1 || true
  if cargo build --release --target "$target"; then
    slices+=("target/$target/release/$BIN_NAME")
  else
    # Homebrew's Rust only ships std for the host. Release builds run in CI on
    # rustup, where both slices are always available.
    say "WARNING: couldn't build $target — the result will not be universal"
  fi
done
[[ ${#slices[@]} -gt 0 ]] || { echo "no architectures built" >&2; exit 1; }

rm -rf "$DIST"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

lipo -create -output "$APP/Contents/MacOS/$BIN_NAME" "${slices[@]}"
chmod +x "$APP/Contents/MacOS/$BIN_NAME"

ARCHS="$(lipo -archs "$APP/Contents/MacOS/$BIN_NAME")"
say "Architectures: $ARCHS"
# Never claim "universal" on a single-slice build — the filename is the only
# thing most people check before downloading.
if [[ "$ARCHS" == *arm64* && "$ARCHS" == *x86_64* ]]; then
  DMG="$DIST/DiscordDesktopFixer-$VERSION-universal.dmg"
else
  DMG="$DIST/DiscordDesktopFixer-$VERSION-${ARCHS// /-}.dmg"
fi

# ---------------------------------------------------------------- bundle
say "Assembling $APP_NAME.app"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

ICONSET="$DIST/icon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z $size $size assets/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) assets/icon.png \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/icon.icns"
rm -rf "$ICONSET"

# ---------------------------------------------------------------- sign
if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  say "Signing with hardened runtime"
  # A single-binary bundle has nothing nested, so there is no need for the
  # deprecated --deep.
  codesign --force --options runtime --timestamp \
    --entitlements packaging/macos/entitlements.plist \
    --sign "$SIGN_IDENTITY" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
else
  # Ad-hoc sign anyway. The linker already ad-hoc signs the executable, so a
  # bundle with no signature of its own is not merely "unsigned" — it is
  # *inconsistent*, and Gatekeeper reports it as damaged ("code has no
  # resources but signature indicates they must be present") rather than
  # untrusted, which loses the user their Open Anyway escape hatch.
  say "SIGN_IDENTITY not set — ad-hoc signing (Gatekeeper will warn on download)"
  codesign --force --sign - "$APP"
  codesign --verify --strict --verbose=2 "$APP"
fi

# ---------------------------------------------------------------- dmg
say "Creating disk image"
STAGE="$DIST/stage"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
# The drag-to-install target people expect.
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format ULFO "$DMG" >/dev/null
rm -rf "$STAGE"

[[ -n "${SIGN_IDENTITY:-}" ]] && codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG"

# ---------------------------------------------------------------- notarize
notarize_args=()
if [[ -n "${NOTARY_PROFILE:-}" ]]; then
  notarize_args=(--keychain-profile "$NOTARY_PROFILE")
elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_PASSWORD:-}" ]]; then
  notarize_args=(--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD")
fi

if [[ ${#notarize_args[@]} -gt 0 && -n "${SIGN_IDENTITY:-}" ]]; then
  say "Submitting to Apple for notarization (this takes a few minutes)"
  xcrun notarytool submit "$DMG" "${notarize_args[@]}" --wait

  # Stapling the ticket is what lets the app open on a machine that is offline
  # or behind a firewall — without it Gatekeeper has to phone home.
  say "Stapling ticket"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"

  say "Verifying Gatekeeper acceptance"
  spctl --assess --type open --context context:primary-signature -vv "$DMG"
else
  say "Notarization credentials not set — skipping (Gatekeeper will warn)"
fi

say "Done: $DMG"
