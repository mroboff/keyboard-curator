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
| Dygma Defy | Dygma's firmware | Live over USB | | | |

## Launch

- [ ] Known limitations are listed in the release notes: what has not been run on hardware, the unverified Imprint LED order if it is still unverified, which firmware is experimental.
- [ ] The version in `Cargo.toml` is bumped, the tag `vX.Y.Z` is pushed, the release workflow attaches the DMG, the release notes are written, and the release is published (not a draft).
- [ ] The README's download link points at the release.
- [ ] Follow-up projects are opened in Linear: more boards, Windows and Linux, local builds.
