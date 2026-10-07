# Contributing

Keyboard Curator is a Rust workspace. This page is the short version of
how it fits together and how to add to it; `CLAUDE.md` holds the rules in
full and is kept current.

## Building and checking

```sh
cargo build --workspace
cargo test --workspace
cargo run -p kc-app
```

On macOS the GUI framework compiles Metal shaders at build time, which
needs the full Xcode app: if `xcrun -f metal` fails, run with
`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`.

Before a commit, the same four checks CI runs:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ci/check-gui-boundary.sh
```

## How the crates fit together

| Crate | Responsibility |
|---|---|
| `kc-model` | The layout (`Project`) and its edit commands with undo, validation, save and load; the user's saved boards and their firmware configuration |
| `kc-zmk` | ZMK keycodes, the behavior catalog, the Kconfig catalog and the curated add-on catalog |
| `kc-boards` | Board definitions, embedded from `crates/kc-boards/boards/*.toml` |
| `kc-emit` | Generates ZMK config files from a layout and a board's firmware configuration |
| `kc-rmk` | RMK: a layout as an RMK keymap and project, or as moergo-rmk's runtime configuration |
| `kc-rynk` | Rynk, RMK's host protocol: read and write a keyboard's configuration over USB |
| `kc-dygma` | Dygma's Focus protocol and the Defy's stored form of a layout |
| `kc-firmware` | The one front door to the firmware families: check, generate, read and apply |
| `kc-import` | Imports `.keymap` files, MoErgo Layout Editor JSON and RMK runtime files |
| `kc-build` | GitHub Actions builds, and firmware fetched from GitHub releases |
| `kc-flash` | UF2 flashing |
| `kc-device` | Finds connected keyboards over USB and recognizes their board |
| `kc-studio` | ZMK Studio RPC over USB |
| `kc-sound` | The key tester's synthesizer |
| `kc-app` | The GPUI application |

Three rules shape everything:

- **Only `kc-app` depends on GPUI.** Logic lives in the other crates, which
  have no UI dependency, so they can be tested on their own and could back
  another front end. `ci/check-gui-boundary.sh` enforces it.
- **A firmware family is code; a firmware within a family is data.** ZMK,
  RMK and Dygma each have a crate. Two ZMK firmwares differ only by their
  profile in a board definition. The app asks `kc-firmware` and never
  branches on the family itself.
- **Boards own the firmware; layouts own the keymap.** A saved board holds
  the firmware choice and every setting; a `.kcproj` layout holds only what
  goes in the keymap. Layers are referenced by stable ID, never by index.

## Adding a board

A board is a TOML file under `crates/kc-boards/boards/`, embedded at build
time and validated by the tests in `crates/kc-boards/tests`. Start from
the board most like yours and work through it:

1. **Identity**: `id`, `name`, `vendor`, `brightness_cap` (a hard limit the
   model enforces), `default_layout`, and `starter_keys`, the plain keys a
   new layout starts with in binding order.
2. **USB**: the vendor and product IDs and product name of the stock
   firmware, so `kc-device` can recognize it. ZMK boards often share IDs;
   the name tells them apart.
3. **Flash**: the order the halves are flashed, how to enter the
   bootloader, and for each half its bootloader volume name and, if the
   vendor uses one, its UF2 family ID.
4. **Physical layouts**: `[[layouts]]`, one per ZMK physical layout, with
   `keys` as `[w, h, x, y, rot, rx, ry]` in ZMK's units and in keymap
   order. Transcribe them from the vendor's `*-layouts.dtsi`; the file's
   header names the source and revision, as the existing boards do.
5. **LEDs**: a `[halves.leds]` chain per half, the key position under each
   LED in chain order, `verified = false` until it has been seen on
   hardware. The app's Lighting mode has LED check patterns for that.
6. **Pointing devices**: `[[pointing]]` entries with the input listener
   label the keymap uses.
7. **Firmware profiles**: one `[[firmware]]` per firmware. For ZMK: the
   Zephyr generation, the ZMK repository and revision, modules, the build
   matrix per half, capabilities (`studio`, `rgb-underglow`,
   `per-key-lighting`, `pointing`, …) and, for per-key lighting, how the
   lighting back end is driven. For RMK: `[firmware.rmk]` with the flavor,
   the pinned source, and either the hardware half of `keyboard.toml` (a
   generated project) or the release to fetch and the matrix, LED and
   pointing tables for configuring the keyboard live. Every firmware
   carries `notes`, the plain sentences the user reads about its limits.
8. **A factory template**: run the `make_template` example in `kc-model`
   on the vendor's keymap and check the result in under
   `crates/kc-model/templates/`, then list it in `project::TEMPLATES`.
9. **Fixtures**: add the board to `crates/kc-emit/tests/fixtures.rs` so a
   generated config is checked in under `fixtures/` and compiled by the
   Firmware workflow (`.github/workflows/firmware.yml`).

Facts taken from other projects are transcribed with their source named in
the file, and listed in `THIRD-PARTY.md`. No GPL code is copied into this
MIT project.

## Adding a firmware to an existing board

Add a `[[firmware]]` profile; no code should change. If it needs code, it
is a new family, which is a bigger conversation: open an issue first.

## Style

American spelling everywhere, in UI text, comments and docs. Comments say
why, in prose. Themes are data files under `crates/kc-app/assets/themes`;
screens take every color and typeface from the theme, never from a literal.
