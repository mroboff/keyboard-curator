#!/usr/bin/env bash
# Packages the release build of kc-app as a macOS app bundle and a DMG.
#
# Usage: ci/bundle-macos.sh <triple> [<triple>...]
#
#   ci/bundle-macos.sh aarch64-apple-darwin
#   ci/bundle-macos.sh aarch64-apple-darwin x86_64-apple-darwin
#
# Each triple names a binary already built at
# target/<triple>/release/keyboard-curator. One triple is copied as is; two
# are joined into a universal binary with lipo. The script writes
#
#   dist/Keyboard Curator.app
#   dist/Keyboard-Curator-<version>-macos.dmg
#
# with the version taken from [workspace.package] in Cargo.toml.
#
# Signing: with MACOS_SIGNING_IDENTITY set (a "Developer ID Application: ..."
# identity in the keychain) the app is signed with the hardened runtime and
# a timestamp, as notarization requires. Without it the app is ad-hoc
# signed, which runs locally but is blocked by Gatekeeper on other Macs
# until the user allows it.
#
# Notarization: with APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD all set
# (and a real signing identity) the DMG is submitted to Apple's notary
# service, and the ticket is stapled to it. Otherwise this step is skipped.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ $# -lt 1 ]; then
    echo "usage: $0 <triple> [<triple>...]" >&2
    exit 2
fi

name="Keyboard Curator"
executable="keyboard-curator"
app="dist/$name.app"
version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "error: no version found under [workspace.package] in Cargo.toml" >&2
    exit 1
fi
dmg="dist/Keyboard-Curator-$version-macos.dmg"

binaries=()
for triple in "$@"; do
    binary="target/$triple/release/$executable"
    if [ ! -x "$binary" ]; then
        echo "error: $binary is missing; build it with" >&2
        echo "  cargo build --release --locked -p kc-app --target $triple" >&2
        exit 1
    fi
    binaries+=("$binary")
done

# The bundle's skeleton.
rm -rf "$app" "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

if [ ${#binaries[@]} -eq 1 ]; then
    cp "${binaries[0]}" "$app/Contents/MacOS/$executable"
    echo "Copied ${binaries[0]}"
else
    lipo -create "${binaries[@]}" -output "$app/Contents/MacOS/$executable"
    echo "Joined ${binaries[*]} into a universal binary"
fi
chmod 755 "$app/Contents/MacOS/$executable"

cp crates/kc-app/Info.plist "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$app/Contents/Info.plist"
cp crates/kc-app/assets/icon/icon.icns "$app/Contents/Resources/icon.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"
echo "Wrote $app (version $version)"

# Signing. Fonts, themes and board data are compiled into the binary, so
# the executable is the only code in the bundle and --deep is not needed.
if [ -n "${MACOS_SIGNING_IDENTITY:-}" ]; then
    codesign --force --options runtime --timestamp \
        --entitlements crates/kc-app/entitlements.plist \
        --sign "$MACOS_SIGNING_IDENTITY" "$app"
    echo "Signed with $MACOS_SIGNING_IDENTITY"
    signed=yes
else
    codesign --force --sign - "$app"
    echo "Ad-hoc signed (set MACOS_SIGNING_IDENTITY to sign with a Developer ID)"
    signed=no
fi
codesign --verify --strict "$app"

# The DMG: the app beside a link to /Applications, so installing is a drag.
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -quiet -volname "$name" -srcfolder "$staging" -ov -format UDZO "$dmg"
echo "Wrote $dmg"

# Notarization, only when it can succeed: Apple rejects ad-hoc signatures.
if [ "$signed" = yes ] && [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$dmg" --wait \
        --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD"
    xcrun stapler staple "$dmg"
    echo "Notarized and stapled $dmg"
else
    echo "Not notarized (needs MACOS_SIGNING_IDENTITY, APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD)"
fi
