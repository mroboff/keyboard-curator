# CLAUDE.md

This file provides context to Claude Code when working in this repository.

## Project Overview

**keyboard-curator** is a native Rust desktop editor for ZMK keyboards: keymaps, layers, every ZMK behaviour, pointing devices, firmware settings and per-key, per-layer lighting, plus building and flashing firmware. v1 targets macOS and two boards: the wireless Cyboard Imprint (82-key) and the MoErgo Go60.

The plan, hardware analyses and issues live in Linear: team `Keyboard-Curator` (key `KEY`), project "Keyboard Curator v1 (macOS)". Read the implementation plan document there before making architectural changes.

## Architecture

A Cargo workspace under `crates/`:

| Crate | Responsibility |
|---|---|
| `kc-model` | Project model and edit commands (undo/redo, validation, save/load); the user's saved keyboards |
| `kc-zmk` | ZMK keycodes, behaviour catalogue, Kconfig catalogue |
| `kc-boards` | Board definitions as embedded data |
| `kc-emit` | Generates the ZMK config files |
| `kc-import` | Imports `.keymap` files and MoErgo Layout Editor JSON |
| `kc-build` | Build back ends (GitHub Actions first) |
| `kc-flash` | UF2 flashing |
| `kc-device` | Finds connected keyboards over USB and recognises their board |
| `kc-studio` | ZMK Studio RPC |
| `kc-app` | GPUI application (binary `keyboard-curator`) |

## Rules

- **Only `kc-app` depends on GPUI.** All logic lives in UI-independent crates. `ci/check-gui-boundary.sh` enforces this.
- **The app's project file is the source of truth**; ZMK config files are generated from it, never edited in place.
- **Layers are referenced by stable ID**, never by index. The emitter resolves IDs to indices.
- **Firmware differences are data** (firmware profiles in board definitions), not code branches.
- **Projects are opened under a saved keyboard** ("My Boards"), which decides the firmware. A project file still records its board and firmware; opening it under a keyboard with another firmware of that board switches it over.
- **What a firmware lacks is hidden, never deleted**: colours, pointing configuration and settings stay in the project, out of the UI and out of the generated config. Key bindings that need a missing feature are flagged instead.
- **A physical device is linked to at most one saved keyboard.** The keyboard store enforces it.
- In code a saved keyboard is a "keyboard"; "profile" means a firmware profile.
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
