#!/usr/bin/env bash
# Packages the release build of kc-app as a Linux tarball.
#
# Usage: ci/bundle-linux.sh [<triple>]
#
#   ci/bundle-linux.sh                             # target/release
#   ci/bundle-linux.sh x86_64-unknown-linux-gnu    # target/<triple>/release
#
# The binary must already be built with `cargo build --release --locked
# -p kc-app` (with `--target <triple>` when one is given). The script
# writes
#
#   dist/Keyboard-Curator-<version>-linux-<arch>.tar.gz
#
# with the version taken from [workspace.package] in Cargo.toml and the
# architecture from the triple, or from `uname -m` without one. The
# tarball unpacks to one folder, keyboard-curator-<version>/, holding
#
#   keyboard-curator               the app
#   keyboard-curator.desktop       the desktop entry
#   keyboard-curator.png           its icon, 256 by 256
#   70-keyboard-curator.rules      udev rules for the keyboards
#   LICENSE, THIRD-PARTY.md        the terms
#   README.txt                     how to install the pieces
#
# Nothing is signed: Linux has no counterpart to notarization, and the
# release's checksums are what a cautious user checks.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ $# -gt 1 ]; then
    echo "usage: $0 [<triple>]" >&2
    exit 2
fi

executable="keyboard-curator"
version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
    echo "error: no version found under [workspace.package] in Cargo.toml" >&2
    exit 1
fi

if [ $# -eq 1 ]; then
    triple="$1"
    binary="target/$triple/release/$executable"
    arch="${triple%%-*}"
else
    triple=""
    binary="target/release/$executable"
    arch=$(uname -m)
fi
if [ ! -x "$binary" ]; then
    echo "error: $binary is missing; build it with" >&2
    echo "  cargo build --release --locked -p kc-app${triple:+ --target $triple}" >&2
    exit 1
fi

folder="keyboard-curator-$version"
tarball="dist/Keyboard-Curator-$version-linux-$arch.tar.gz"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
mkdir -p "dist" "$staging/$folder"
rm -f "$tarball"

cp "$binary" "$staging/$folder/$executable"
chmod 755 "$staging/$folder/$executable"
cp packaging/linux/keyboard-curator.desktop "$staging/$folder/"
cp crates/kc-app/assets/icon/icon-256.png "$staging/$folder/keyboard-curator.png"
cp packaging/linux/70-keyboard-curator.rules "$staging/$folder/"
cp LICENSE THIRD-PARTY.md "$staging/$folder/"
cat > "$staging/$folder/README.txt" <<README
Keyboard Curator $version for Linux ($arch)

A keymap, feature and lighting editor for programmable keyboards.
Source and releases: https://github.com/mroboff/keyboard-curator

Install
-------

1. The app. Put keyboard-curator somewhere on your PATH, for yourself:

       install -Dm755 keyboard-curator ~/.local/bin/keyboard-curator

   or for everyone:

       sudo install -Dm755 keyboard-curator /usr/local/bin/keyboard-curator

   It is one file; fonts, themes and board data are inside it. It needs
   Vulkan, Wayland or X11, fontconfig, ALSA and libudev, which every
   desktop distribution has.

2. The launcher and icon, so the app appears in your desktop's menu:

       install -Dm644 keyboard-curator.desktop ~/.local/share/applications/keyboard-curator.desktop
       install -Dm644 keyboard-curator.png ~/.local/share/icons/hicolor/256x256/apps/keyboard-curator.png

   (Under /usr/local/share for everyone.) The desktop entry expects the
   app on PATH; edit its Exec line if you put it elsewhere.

3. Access to the keyboards. The app talks to a keyboard over its serial
   port and its raw HID interface, which belong to root until udev says
   otherwise:

       sudo cp 70-keyboard-curator.rules /etc/udev/rules.d/
       sudo udevadm control --reload-rules && sudo udevadm trigger

   then unplug the keyboard and plug it back in. The rules grant the
   logged-in user access. Without them, the serial port alone works for
   members of the dialout group (Debian, Ubuntu) or uucp (Arch).

4. Flashing. A keyboard half in its bootloader shows up as a small drive.
   Most desktops mount it on their own under /run/media/<you> or
   /media/<you>; if yours does not, mount it anywhere and the app finds it.

Uninstall by removing the files installed above. The app keeps its own
settings under ~/.config/keyboard-curator and its layouts where you save
them, by default in ~/Documents/Keyboard Curator.
README

tar -C "$staging" -czf "$tarball" "$folder"
echo "Wrote $tarball (version $version, $arch)"
