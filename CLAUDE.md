# CLAUDE.md

This file provides context to Claude Code when working in this repository.

## Project Overview

**keyboard-curator** is a native Rust desktop editor for ZMK keyboards: keymaps, layers, every ZMK behavior, pointing devices, firmware settings and per-key, per-layer lighting, plus building and flashing firmware. v1 targets macOS and two boards: the wireless Cyboard Imprint (82-key) and the MoErgo Go60.

The plan, hardware analyses and issues live in Linear: team `Keyboard-Curator` (key `KEY`), project "Keyboard Curator v1 (macOS)". Read the implementation plan document there before making architectural changes.

## Architecture

A Cargo workspace under `crates/`:

| Crate | Responsibility |
|---|---|
| `kc-model` | Layout model and edit commands (undo/redo, validation, save/load); the user's saved boards and their firmware configuration |
| `kc-zmk` | ZMK keycodes, behavior catalog, Kconfig catalog |
| `kc-boards` | Board definitions as embedded data |
| `kc-emit` | Generates the ZMK config files |
| `kc-import` | Imports `.keymap` files and MoErgo Layout Editor JSON |
| `kc-build` | Build back ends (GitHub Actions first) |
| `kc-flash` | UF2 flashing |
| `kc-device` | Finds connected keyboards over USB and recognizes their board |
| `kc-studio` | ZMK Studio RPC |
| `kc-app` | GPUI application (binary `keyboard-curator`) |

## Rules

- **Only `kc-app` depends on GPUI.** All logic lives in UI-independent crates. `ci/check-gui-boundary.sh` enforces this.
- **The app's own files are the source of truth** (layout files and the saved boards); ZMK config files are generated from them, never edited in place.
- **Layers are referenced by stable ID**, never by index. The emitter resolves IDs to indices.
- **Firmware differences are data** (firmware profiles in board definitions), not code branches.
- **Boards own the firmware; layouts own the keymap.** A saved board ("My Boards") holds the firmware choice, every firmware setting, custom `.conf` lines, the build repository, the linked device and its list of layouts with one current. A layout file (`.kcproj`, the `Project` type) holds only what goes in the `.keymap`. Nothing about the firmware goes in a layout, and changing a board never touches a layout file.
- **Firmware work happens on the board's page** (settings, Build & Flash, Export); the layout editor only edits a layout and applies it to its board.
- **What a firmware lacks is hidden, never deleted**: colors and pointing configuration stay in the layout, and settings stay on the board, out of the UI and out of the generated config. Key bindings that need a missing feature are flagged instead.
- **A physical device is linked to at most one saved board.** The store enforces it.
- In code a saved board is a "keyboard" and a layout is a "project"; "profile" means a firmware profile. The UI says "board" and "layout".
- **American spelling everywhere**: UI text, messages, comments and docs (behavior, color, recognize, center, catalog). The exceptions are other people's names for things, such as GitHub's `cancelled` run conclusion and vendored ZMK files.
- **Board limits are hard limits**: LED brightness is capped at 40 on the Go60 (warranty) and 50 on the Imprint.

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
