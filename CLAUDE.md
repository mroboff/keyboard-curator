# CLAUDE.md

This file provides context to Claude Code when working in this repository.

## Project Overview

**keyboard-curator** is a native Rust desktop editor for programmable keyboards: keymaps, layers, behaviors, pointing devices, firmware settings and per-key, per-layer lighting, plus building and flashing firmware or writing a configuration straight to the keyboard. v1 targets macOS and Linux and four boards: the wireless Cyboard Imprint (82-key), the MoErgo Go60 and the MoErgo Glove80, on ZMK or RMK, and the Dygma Defy on Dygma's own firmware.

The plan, hardware analyses and issues live in Linear: team `Keyboard-Curator` (key `KEY`), project "Keyboard Curator v1 (macOS)". Read the implementation plan document there before making architectural changes.

## Architecture

A Cargo workspace under `crates/`:

| Crate | Responsibility |
|---|---|
| `kc-model` | Layout model and edit commands (undo/redo, validation, save/load); the user's saved boards and their firmware configuration |
| `kc-zmk` | ZMK keycodes, behavior catalog, Kconfig catalog, and the catalog of ZMK add-ons |
| `kc-boards` | Board definitions as embedded data |
| `kc-emit` | Generates the ZMK config files |
| `kc-rmk` | RMK: layouts as RMK keymaps and the project that builds them, or as moergo-rmk's runtime configuration, with that firmware's settings |
| `kc-rynk` | Rynk, RMK's host protocol: a keyboard's configuration read and written over USB HID, bootloader entry and settings reset |
| `kc-dygma` | Dygma: the Focus protocol, and layouts to and from the keyboard's stored form |
| `kc-firmware` | The app's one front door to the firmware families: check, generate, read and apply |
| `kc-import` | Imports `.keymap` files, MoErgo Layout Editor JSON and RMK runtime configuration files |
| `kc-build` | Build back ends (GitHub Actions first), and firmware fetched from GitHub releases |
| `kc-flash` | UF2 flashing |
| `kc-device` | Finds connected keyboards over USB and recognizes their board |
| `kc-studio` | ZMK Studio RPC |
| `kc-sound` | The key tester's synthesizer and sound output |
| `kc-app` | GPUI application (binary `keyboard-curator`) |

## Rules

- **Only `kc-app` depends on GPUI.** All logic lives in UI-independent crates. `ci/check-gui-boundary.sh` enforces this.
- **The app's own files are the source of truth** (layout files and the saved boards); ZMK config files are generated from them, never edited in place.
- **Layers are referenced by stable ID**, never by index. The emitter resolves IDs to indices.
- **A firmware family is code; a firmware within a family is data.** ZMK, RMK and Dygma are families, each with its own crate. Firmwares within a family differ only by their profile in a board definition, never by code branches. The app asks `kc-firmware`, and does not branch on the family itself.
- **Families deliver in one of three ways** (`Delivery`, per firmware profile): ZMK and RMK generate files that are built and flashed; Dygma is configured on the running keyboard, with no build; moergo-rmk is taken ready-made from a pinned GitHub release, checked against its checksums, flashed once, and then configured on the running keyboard over Rynk. The board page treats all three as first-class.
- **Rynk is version-matched.** The `rynk` and `moergo-config` crates are git dependencies pinned to the same moergo-rmk commit the MoErgo releases are built from, and a fetched release's manifest must name the board profile's source commit and that RMK commit. Moving the pin means moving the release tags, and the other way round. The Imprint's firmware is a fork of moergo-rmk (a board crate `imprint-rmk` beside the MoErgo ones) that keeps the same RMK submodule, so one Rynk client serves all three boards.
- **RMK's settings are keyed `rmk.<section>.<name>`** (catalog in `kc-rmk/src/settings.rs`) and live in the board's `FirmwareConfig` like Kconfig options do. A setting left at its default is not written to the keyboard: what the keyboard holds for it stays.
- **Dygma's settings are keyed `dygma.<section>.<name>`** (catalog in `kc-dygma/src/settings.rs`), each standing for one Focus command. Reading a keyboard carries them to the board; applying writes only the ones that differ, in the words the keyboard used, and only the commands the keyboard lists in `help`. A wired Defy has no wireless settings and is never sent one.
- **Layouts are stored in one vocabulary**, the structured bindings that began as ZMK's. Each family translates them and reports what it cannot express at the key; nothing is dropped silently.
- **Keys are edited as slots, never as definitions.** `kc-model::setup::KeySetup` reads a key as Tap, Hold, Double tap, Tap then hold, More taps, With ⇧ held, Sticky and a feel, and `Project::apply_setup` writes it back, choosing a built-in (`mt`, `lt`, `sk`, `sl`), a template reference, or a key-owned definition. A `BehaviorDef` is a **template** (`Scope::Template`, named by the user, listed on the Templates page) or **key-owned** (`Scope::Key`, auto-labeled, hidden, merged when identical and dropped when unused by `Project::tidy`, which the editor runs after every edit). A key-owned definition may `follow` a template for its settings. The UI never creates or edits a `BehaviorDef` directly; it goes through `KeySetup` and the template methods on `Project`.
- **The app speaks the glossary**, `kc-model::words`: Hold & tap, Multi-tap, Shift override, Sticky, Macro; "Nav while held", "Switch to Nav"; ⌃ ⌥ ⇧ ⌘. ZMK labels, `&` text and devicetree appear only under Advanced and in Generated Files.
- **Writing to a keyboard is strict**: a fixed list of commands, payloads of exactly the keyboard's size, only what differs, and a backup of what the keyboard held first. Never widen the list casually.
- **A Dygma backup is everything the keyboard lists** under `help`, read as the keyboard wrote it, minus Bazecor's own skip list (`BACKUP_SKIP` in `kc-dygma`: actions, probes, the factory keymap). A restore writes a backup back to the keyboard it came from, command by command, only what differs, and never `led.*` or `wireless.*`, which Bazecor's restore leaves alone too.
- **The Curator firmware is a second Dygma profile, data only.** The owner's fork of Dygma's 2.2.1 Defy wireless firmware (`mroboff/NeuronWireless_defy` branch `curator`, with `mroboff/NeuronWirelessLibs` and `mroboff/Kaleidoscope-Dygma`, all GPL-3.0, nothing linked in) adds combos and per-key hold and tap timing. It is the `dygma-defy-curator` profile in the Defy board file, with the capabilities `combos` and `per-key-timing` and a `fork` table (`DygmaFork`: release repository and tag, source and Kaleidoscope commits, the version mark, the two release files). The fork's Focus commands and their formats are facts in `kc-dygma/src/chords.rs` (`chords.map`, `chords.timeout`, `chords.memory`) and `kc-dygma/src/timing.rs` (`timing.map`, `timing.capacity`, `timing.defaults`), with attribution. `help` gates everything: the tables are read and written only where the keyboard lists the commands, carried by backups between keyboards that both have them, and a layout needing combos or per-key timing on a keyboard running stock firmware is refused with the firmware named (`Image::firmware_name`). `kc_dygma::check`, `with_project`, `apply` and `expressible` take the profile's features; a custom feel stays an error without `per-key-timing`.
- **VIA keyboards are described by their definition file.** `kc-via` speaks VIA's protocol over raw HID and reads a VIA v3 definition into a board (`Family::Via`, delivered live, whose profile's `via` section holds each key's matrix position and the lighting values the definition reaches). Definitions are loaded from a file the user chooses, never bundled (VIA's collection is GPL-3.0). QMK keycodes map to bindings in `kc-via/src/codec.rs`; a keycode the app cannot name is kept as a raw binding and written back unchanged. Settings are keyed `via.<channel>.<name>` and offered only where the definition reaches the channel. The board page's live panels take their words from `kc_firmware::live::words`, never from the app branching on the family.
- **Third-party facts carry their source.** Board data taken from other projects (Dygma's Bazecor, vendors' ZMK files, moergo-rmk) is transcribed as facts with attribution in the file. No GPL code is copied into this MIT project.
- **Boards own the firmware; layouts own the keymap.** A saved board ("My Boards") holds the firmware choice, every firmware setting, custom `.conf` lines, the build repository, the linked device and its list of layouts with one current. A layout file (`.kcproj`, the `Project` type) holds only what goes in the `.keymap`. Nothing about the firmware goes in a layout, and changing a board never touches a layout file.
- **Firmware work happens on the board's page** (settings, Build & Flash, Export); the layout editor only edits a layout and applies it to its board.
- **What a firmware lacks is hidden, never deleted**: colors and pointing configuration stay in the layout, and settings stay on the board, out of the UI and out of the generated config. Key bindings that need a missing feature are flagged instead.
- **A physical device is linked to at most one saved board.** The store enforces it.
- In code a saved board is a "keyboard" and a layout is a "project"; "profile" means a firmware profile. The UI says "board" and "layout".
- **American spelling everywhere**: UI text, messages, comments and docs (behavior, color, recognize, center, catalog). The exceptions are other people's names for things, such as GitHub's `cancelled` run conclusion and vendored ZMK files.
- **Platform differences live behind `cfg(target_os)` in the smallest possible functions**: where removable drives mount (`kc_flash::volume_roots`), where the menus go (`install_menus`), how the window is opened (`window_options`). The UI never branches on the platform except for the shortcuts (`secondary-` bindings, spelled ⌘ on a Mac and Ctrl elsewhere by `shortcut`) and the menu bar, which the Shell draws in the window's own title bar where the system has none. The Linux packaging (desktop entry, udev rules) is under `packaging/linux`, and `shell.nix` builds the app on NixOS.
- **A theme is data.** Each theme is a file in `crates/kc-app/assets/themes`: the toolkit's theme for light, dark or both, plus a "look" (keycap style, plinth and key colors, display typeface). Screens take every color, typeface and shape from the toolkit theme and `theme::look`, never from literals, and build with the shared pieces in `workspace.rs` (`chip`, `tab`, `badge`, `display`, `heading`, `subheading`, `help`, `card`, `group`, `field`, `plinth`) rather than styling text and boxes by hand. A theme with both light and dark follows View › Appearance; a theme with one stays that way. Typefaces a theme names are bundled under `assets/fonts` with their licenses.
- **A board is a file, and a shield names its controller.** Every `.toml` under `crates/kc-boards/boards` (and its subdirectories) is embedded by the build script; adding a board needs no code. A keyboard that is a shield has one firmware profile per controller it fits, each naming the controller from `crates/kc-boards/controllers.toml`, which carries the bootloader volume, UF2 family and how to enter the bootloader; the halves then carry none. A keyboard that is not split has one `whole` half. A board nobody has plugged in is `verified = false`, and the app says so when it is chosen. The boards under `boards/zmk` are generated from ZMK's own tree by the `zmk_boards` example in `kc-import` (rerun it against a checkout at the pinned tag rather than editing them); the four hand-written boards sit beside them. A keyboard the app has no file for can still be added from what it reports over ZMK Studio (`Board::from_studio`): a whole board with one `studio_only` profile, delivered as `Delivery::Studio` (layers and keys written live, nothing built, no settings), saved with the user's keyboards in `keyboards.json` rather than embedded.
- **Board limits are hard limits**: LED brightness is capped at 40 on the Go60 and 80 on the Glove80 (warranty) and 50 on the Imprint.
- **Starter layouts are data with credit.** `kc-model/templates` holds a board's factory layout and any community layout brought to it, listed in `project::TEMPLATES` (the layout `Template` type, not to be confused with behavior templates) with the author and a link; factory layouts are regenerated with the `make_template` example, the TailorKey port with kc-import's `tailorkey_imprint` example from Moosy's published files. A community layout ships only with its author's credit, and its redistribution terms are confirmed with the author.
- **ZMK add-ons are curated.** `crates/kc-zmk/catalog/addons.toml` is written by hand; `addons-meta.json` beside it is generated by `ci/refresh-addons.py`. An add-on is offered only pinned to a commit, with its notes.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ci/check-gui-boundary.sh
cargo run -p kc-app
```

## Workflow

- While the initial app is being built, milestone work is committed and pushed straight to `main`. Each commit message is prefixed with its Linear issue key (for example `KEY-3: ...`).
- CI runs fmt, clippy (warnings denied), tests and the GUI boundary check on macOS and on Linux (Ubuntu 24.04). On NixOS, run every cargo command through `nix-shell --run '...'`.
