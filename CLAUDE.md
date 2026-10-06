# CLAUDE.md

This file provides context to Claude Code when working in this repository.

## Project Overview

**keyboard-curator** is a native Rust desktop editor for programmable keyboards: keymaps, layers, behaviors, pointing devices, firmware settings and per-key, per-layer lighting, plus building and flashing firmware or writing a configuration straight to the keyboard. v1 targets macOS and three boards: the wireless Cyboard Imprint (82-key) and the MoErgo Go60, on ZMK or RMK, and the Dygma Defy on Dygma's own firmware.

The plan, hardware analyses and issues live in Linear: team `Keyboard-Curator` (key `KEY`), project "Keyboard Curator v1 (macOS)". Read the implementation plan document there before making architectural changes.

## Architecture

A Cargo workspace under `crates/`:

| Crate | Responsibility |
|---|---|
| `kc-model` | Layout model and edit commands (undo/redo, validation, save/load); the user's saved boards and their firmware configuration |
| `kc-zmk` | ZMK keycodes, behavior catalog, Kconfig catalog, and the catalog of ZMK add-ons |
| `kc-boards` | Board definitions as embedded data |
| `kc-emit` | Generates the ZMK config files |
| `kc-rmk` | RMK: layouts as RMK keymaps, and the project that builds them |
| `kc-dygma` | Dygma: the Focus protocol, and layouts to and from the keyboard's stored form |
| `kc-firmware` | The app's one front door to the firmware families: check, generate, read and apply |
| `kc-import` | Imports `.keymap` files and MoErgo Layout Editor JSON |
| `kc-build` | Build back ends (GitHub Actions first) |
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
- **Families deliver in one of two ways**: ZMK and RMK generate files that are built and flashed; Dygma is configured on the running keyboard, with no build. The board page treats both as first-class.
- **Layouts are stored in one vocabulary**, the structured bindings that began as ZMK's. Each family translates them and reports what it cannot express at the key; nothing is dropped silently.
- **Writing to a keyboard is strict**: a fixed list of commands, payloads of exactly the keyboard's size, only what differs, and a backup of what the keyboard held first. Never widen the list casually.
- **Third-party facts carry their source.** Board data taken from other projects (Dygma's Bazecor, vendors' ZMK files, moergo-rmk) is transcribed as facts with attribution in the file. No GPL code is copied into this MIT project.
- **Boards own the firmware; layouts own the keymap.** A saved board ("My Boards") holds the firmware choice, every firmware setting, custom `.conf` lines, the build repository, the linked device and its list of layouts with one current. A layout file (`.kcproj`, the `Project` type) holds only what goes in the `.keymap`. Nothing about the firmware goes in a layout, and changing a board never touches a layout file.
- **Firmware work happens on the board's page** (settings, Build & Flash, Export); the layout editor only edits a layout and applies it to its board.
- **What a firmware lacks is hidden, never deleted**: colors and pointing configuration stay in the layout, and settings stay on the board, out of the UI and out of the generated config. Key bindings that need a missing feature are flagged instead.
- **A physical device is linked to at most one saved board.** The store enforces it.
- In code a saved board is a "keyboard" and a layout is a "project"; "profile" means a firmware profile. The UI says "board" and "layout".
- **American spelling everywhere**: UI text, messages, comments and docs (behavior, color, recognize, center, catalog). The exceptions are other people's names for things, such as GitHub's `cancelled` run conclusion and vendored ZMK files.
- **A theme is data.** Each theme is a file in `crates/kc-app/assets/themes`: the toolkit's theme for light, dark or both, plus a "look" (keycap style, plinth and key colors, display typeface). Screens take every color, typeface and shape from the toolkit theme and `theme::look`, never from literals, and build with the shared pieces in `workspace.rs` (`chip`, `tab`, `badge`, `display`, `heading`, `subheading`, `help`, `card`, `group`, `field`, `plinth`) rather than styling text and boxes by hand. A theme with both light and dark follows View › Appearance; a theme with one stays that way. Typefaces a theme names are bundled under `assets/fonts` with their licenses.
- **Board limits are hard limits**: LED brightness is capped at 40 on the Go60 (warranty) and 50 on the Imprint.
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
- CI runs fmt, clippy (warnings denied), tests and the GUI boundary check on macOS.
