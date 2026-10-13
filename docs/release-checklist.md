# Release checklist

The manual checks a release goes through, on real keyboards, before it is
tagged. Each board's firmware families are checked separately, because a
layout reaches the keyboard a different way in each. Tick what passed,
note the firmware version and the app commit, and keep the filled-in copy
with the release notes.

Nothing below has been run yet: as of 2026-10-07 the app has not flashed
or configured any keyboard apart from reading a Dygma Defy over USB.

## Before the hardware

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `ci/check-gui-boundary.sh` pass.
- [ ] The Firmware workflow is green on `main` for every fixture (the generated configs compile with the real ZMK toolchain).
- [ ] `ci/check-theme-contrast.py` passes.
- [ ] `THIRD-PARTY.md` is regenerated (`ci/third-party.py`) and the TailorKey template's redistribution terms are confirmed with its author.
- [ ] The user guide (`docs/user-guide`) builds and its screenshots match the app.
- [ ] The release DMG is built by the release workflow, signed and notarized, and installs on a clean machine (a fresh user account is enough) without a Gatekeeper warning.
- [ ] The Linux tarball is built by the release workflow and runs on a clean Ubuntu and a clean Fedora (a fresh virtual machine is enough): the window opens on Wayland and on X11 with its menus in the title bar, the desktop entry and icon show in the application menu, and with the udev rules installed a connected keyboard is recognized and read without root. A bootloader drive that the desktop mounts is found by Build & Flash.

## Every board, every firmware

Repeat this block for each row of the table at the end.

**Board page**
- [ ] Add Board recognizes the connected keyboard over USB (model and name); Add Board without a keyboard works too.
- [ ] The firmware list offers what the board definition says, and the notes read correctly.
- [ ] Settings: change the keyboard name, a brightness setting, a timeout; they appear in the generated files or are applied live.
- [ ] Key Tester: every key lights when pressed; a note sounds.

**Layout editor**
- [ ] New Layout from the factory template, and from every other template the board offers.
- [ ] Keys: assign plain keys, modifiers, a layer key, a Bluetooth key, a mouse key; copy and paste; undo and redo through all of it.
- [ ] Layers: add, rename, reorder, duplicate, delete; references follow the layer.
- [ ] Behaviors: a hold-tap, a tap-dance, a mod-morph, a sticky key, a macro, each bound to a key.
- [ ] Combos: one combo that fires on the keyboard.
- [ ] Pointing (boards with a trackball or touchpad): speed, scroll, invert, a layer that switches on while the device moves.
- [ ] Lighting (per-key firmware only): paint colors on three layers; a lock light; a battery light; "Color by what keys do"; the LED check patterns by row and by column show the right keys.
- [ ] Advanced: a layer rule; a custom devicetree snippet.
- [ ] Apply: the layout becomes the board's current one.
- [ ] Save, close, reopen; the file round-trips without a validation problem.

**Reaching the keyboard**
- [ ] Build & Flash (ZMK and RMK builds): the build runs on GitHub and finishes; each half flashes in the order the app gives; the keyboard comes back with the layout.
- [ ] Keyboard tab (live firmware: moergo-rmk, imprint-rmk, Dygma): Read shows the keyboard's own layout; Apply writes the layout and a backup is written first; Reset settings works; Enter bootloader works for each half.
- [ ] ZMK Studio (ZMK firmware with Studio): "Send key changes now" changes a key on the keyboard without a build; the keyboard asks to be unlocked when it must.
- [ ] Import: the keyboard's current `.keymap` (or Layout Editor export, or RMK `.toml`) imports with no raw bindings; firmware settings in it are offered to the board.
- [ ] Bluetooth: pair to two computers on two profiles and switch between them with the layout's keys.
- [ ] Per-key lighting: colors show on both halves; the brightness cap holds (Go60 40, Glove80 80, Imprint 50); lock and battery lights change state.
- [ ] Recovery: pull the USB cable during a flash, then flash again; the keyboard recovers. Flash the vendor's own firmware back and confirm the keyboard is as it was.

**A keyboard added from ZMK Studio** (Add Connected Board with a keyboard whose name the app does not know, Studio unlocked): the offered model names the keyboard and its key count; the editor shows its keys where they are; Read From Keyboard brings the layers and keys; a changed key applies and survives the keyboard being unplugged; a backup layout file is written first. Try it with an Imprint, Go60 or Glove80 whose USB name has been changed, so the app does not recognize it.

**A QMK keyboard with VIA** (VIA closed throughout; the keyboard on its cable): Add a VIA Keyboard… with the maker's definition file; the keys in the editor sit where they do on the keyboard; Read From Keyboard brings every layer, a macro and the lighting values; a changed key, a new macro and a changed brightness apply and survive the keyboard being unplugged; a backup file is written first and restores. Try a key the app cannot name: it reads as a number and writes back unchanged.

**ZMK shields** (the boards under `crates/kc-boards/boards/zmk`): none is covered by this checklist. They ship unverified and say so; a shield is struck off that list only once someone has run "Every board, every firmware" on it with a real keyboard and its controller.

**Dygma Defy only** (Bazecor closed throughout; reopen it only where it says)
- [ ] Before anything: take a backup in Bazecor too, so the keyboard can be put back without this app.
- [ ] Read From Keyboard: the layout matches what Bazecor shows, layer by layer, including a superkey (its five actions in the key's slots), a macro (its steps, with any delay as a pause), the colors under the keys and the underglow. The board's settings offered on reading match Bazecor's Preferences.
- [ ] Edit a superkey: change one action, add a new superkey on another key, apply. In Bazecor, both read as expected and the keyboard behaves accordingly. The superkey table has grown by exactly one.
- [ ] Edit a macro: change a step, add a pause, apply; the macro plays correctly on the keyboard, and Bazecor shows the delay.
- [ ] Paint the underglow on one layer and apply; the ring lights as painted, in the drawn order (the LED check's repeating run goes around the strip without a break). Unpainted lights keep their color.
- [ ] Change a setting (a typing timeout, then a light's brightness, then a wireless one on a wireless Defy) and apply; each takes effect and shows in Bazecor's Preferences. Settings left alone stay as they were.
- [ ] Restore a Backup…: pick the backup taken before the first write; the keyboard returns to how it was, except lights and wireless settings, which stay. Bazecor agrees.
- [ ] On a wired Defy: nothing wireless is read, shown or written.

**Dygma Defy, Curator firmware only** (a wireless Defy; Bazecor closed except for its firmware update)
- [ ] First, a stock build: flash a build of Dygma's own `N2_Defy_v2.2.1` tag from the fork's toolchain with Bazecor's firmware update (custom firmware folder: that `Wireless_neuron.hex` with Dygma's `keyscanner.bin`), confirm the keyboard comes back, that Read From Keyboard shows `version` as v2.2.1, and that keys, superkeys, macros, colors and settings are as they were.
- [ ] Go back: Bazecor's firmware update with Dygma's own release. Everything survives.
- [ ] Then the fork: the same update with the release's `Wireless_neuron.hex` and `keyscanner.bin`. Read From Keyboard says the keyboard runs the Curator firmware v2.2.1-curator.1; keys, superkeys, macros, colors and settings are as they were; the Combos mode and a key's own feel are offered with the Curator firmware chosen on the board.
- [ ] Make a two-key combo sending Escape, apply, and confirm it fires on the keyboard; a combo on one layer fires there only; Combo timeout on the board's Settings tab changes how far apart the keys may land.
- [ ] Set two home-row keys to different hold times (say 150 ms and 400 ms) and apply; feel the difference. A key with a second tap and its own time between taps behaves accordingly.
- [ ] Read From Keyboard brings the combos and the keys' feels back as they were set; applying that layout unchanged writes nothing.
- [ ] Restore a Backup… taken on the Curator firmware brings combos and timing back after they were erased.
- [ ] Go back to stock with Bazecor's firmware update and confirm everything survived: keys, superkeys, macros, colors and settings. A layout with combos is refused on Dygma's firmware, with the firmware named.

## The matrix

| Board | Firmware | How the layout gets there | Checked by | Date | Result |
|---|---|---|---|---|---|
| Cyboard Imprint | ZMK v0.3.0 with Cyboard's module | Build & Flash | | | |
| Cyboard Imprint | ZMK v0.3.0 with per-key lighting (experimental) | Build & Flash | | | |
| Cyboard Imprint | RMK 0.9 | Build & Flash | | | |
| Cyboard Imprint | RMK for the Imprint (community fork) | Release firmware, then live over Rynk | | | |
| MoErgo Go60 | MoErgo ZMK | Build & Flash | | | |
| MoErgo Go60 | ZMK with community per-key lighting | Build & Flash | | | |
| MoErgo Go60 | RMK for MoErgo (community) | Release firmware, then live over Rynk | | | |
| MoErgo Glove80 | MoErgo ZMK | Build & Flash | | | |
| MoErgo Glove80 | ZMK with community per-key lighting | Build & Flash | | | |
| MoErgo Glove80 | RMK for MoErgo (community) | Release firmware, then live over Rynk | | | |
| Dygma Defy wireless | Dygma's firmware | Live over USB | | | |
| Dygma Defy wireless | Curator firmware v2.2.1-curator.1 | Bazecor custom firmware, then live over USB | | | |
| Dygma Defy wired | Dygma's firmware | Live over USB | | | |

## Launch

- [ ] Known limitations are listed in the release notes: what has not been run on hardware, the unverified Imprint LED order if it is still unverified, which firmware is experimental.
- [ ] The version in `Cargo.toml` is bumped, the tag `vX.Y.Z` is pushed, the release workflow attaches the DMG and the Linux tarball, the release notes are written, and the release is published (not a draft).
- [ ] The README's download link points at the release.
- [ ] Follow-up projects are opened in Linear: more boards, Windows, local builds.
