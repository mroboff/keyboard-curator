# keyboard-curator

A native, open-source keymap, feature and per-key lighting editor for ZMK keyboards.

**Status: early development.** Keymap and behavior editing, per-key lighting, import, config generation, GitHub builds, guided flashing and direct updates over ZMK Studio are all built. The generated firmware configs are compiled in CI, but little has been tried on real keyboards yet.

The first release targets macOS and two boards: the Cyboard Imprint (82-key, wireless) and the MoErgo Go60. More ZMK boards, Windows and Linux are planned afterward.

## Building

Requires the stable Rust toolchain (pinned in `rust-toolchain.toml`) and, on macOS, the full Xcode app: the GUI framework compiles Metal shaders at build time, which the Command Line Tools alone cannot do. If `xcrun -f metal` fails, either run `sudo xcode-select -s /Applications/Xcode.app` or set `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` when building.

```sh
cargo build --workspace
cargo test --workspace
cargo run -p kc-app
```

## Using it

```sh
cargo run -p kc-app                      # My Boards
cargo run -p kc-app -- my-layout.kcproj  # open a layout
```

Configuration is in two levels. A **board** is one of your keyboards: its firmware and that firmware's settings. A **layout** is what the keys do, and a board can have any number of them.

- **My Boards**: the welcome screen lists your boards. A board does not need to be connected, or even owned; one that is on USB can be linked so the app recognizes it.
- **A board's page** has three parts. *Firmware* chooses the firmware and adjusts its settings. *Layouts* creates, opens and imports layouts, and marks the current one. *Build & Flash* pushes the config to a firmware repository on GitHub, waits for the build and flashes each half; it builds the board's settings with the current layout, or the factory layout if none has been applied. Building needs the GitHub CLI signed in (`gh auth login`) or a `GH_TOKEN`.
- **The layout editor** edits one layout: keys, layers, behaviors, combos, per-key colors, pointing, layer rules. It shows only what the board's firmware supports. *Apply* makes the layout the board's current one; with firmware built for ZMK Studio it can also send key changes over USB without a build.
- **Import a Keymap**, on a board's page, opens an existing `.keymap` file or a MoErgo Layout Editor export as a new layout. Firmware settings found with it are offered to the board.

`fixtures/` holds a generated config for each board. CI builds them with the real ZMK toolchain; after an intended change to the emitter, refresh them with `UPDATE_FIXTURES=1 cargo test -p kc-emit --test fixtures`.

## Layout

A Cargo workspace under `crates/`. Only `kc-app` may depend on the GUI framework; CI enforces this with `ci/check-gui-boundary.sh`.

| Crate | Responsibility |
|---|---|
| `kc-model` | The layout: layers, bindings, behaviors, combos, macros, pointing, lighting; edit commands with undo/redo. Also the user's saved boards and their firmware settings |
| `kc-zmk` | ZMK knowledge as data: keycodes, behavior catalog, Kconfig options |
| `kc-boards` | Board definitions: physical layouts, LED maps, firmware profiles |
| `kc-emit` | Generates `.keymap`, `.conf`, `west.yml` and `build.yaml` |
| `kc-import` | Imports existing `.keymap` files and MoErgo Layout Editor JSON |
| `kc-build` | Firmware build back ends (GitHub Actions first) |
| `kc-flash` | UF2 bootloader detection and flashing |
| `kc-device` | Finds connected keyboards over USB and recognizes their board |
| `kc-studio` | ZMK Studio transport and RPC for live editing |
| `kc-app` | The desktop application |

## Contributing

Before sending a change, run what CI runs:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ci/check-gui-boundary.sh
```

## License

MIT. See `LICENSE`.
