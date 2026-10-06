# keyboard-curator

A native, open-source keymap, feature and per-key lighting editor for ZMK keyboards.

**Status: early development.** Keymap and behaviour editing, per-key lighting, import, config generation, GitHub builds, guided flashing and direct updates over ZMK Studio are all built. The generated firmware configs are compiled in CI, but little has been tried on real keyboards yet.

The first release targets macOS and two boards: the Cyboard Imprint (82-key, wireless) and the MoErgo Go60. More ZMK boards, Windows and Linux are planned afterwards.

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
cargo run -p kc-app -- my-layout.kcproj  # open a project
```

- **My Boards**: the welcome screen lists your keyboards. Each has a board model and the firmware it runs, and can be linked to the physical keyboard on USB so the app recognises it. A keyboard does not need to be connected, or even owned. Projects are created, opened and imported under a keyboard, and the editor shows only what that keyboard's firmware supports.
- **Keyboard**: select keys, pick bindings, edit layers.
- **Generated Files**: the zmk-config files the project produces, and any problems.
- **Lighting, Behaviors, Combos, Pointing, Settings**: per-key colours, hold-taps, macros and the rest.
- **File > Import Keymap**: opens an existing `.keymap` file or a MoErgo Layout Editor export as a new project.
- **Build & Flash**: pushes the config to a firmware repository on GitHub, waits for the build and flashes each half. With firmware built for ZMK Studio, it can also send key changes over USB without a build. Building needs the GitHub CLI signed in (`gh auth login`) or a `GH_TOKEN`.

`fixtures/` holds a generated config for each board. CI builds them with the real ZMK toolchain; after an intended change to the emitter, refresh them with `UPDATE_FIXTURES=1 cargo test -p kc-emit --test fixtures`.

## Layout

A Cargo workspace under `crates/`. Only `kc-app` may depend on the GUI framework; CI enforces this with `ci/check-gui-boundary.sh`.

| Crate | Responsibility |
|---|---|
| `kc-model` | The project: layers, bindings, behaviours, combos, macros, pointing, lighting, settings; edit commands with undo/redo. Also the user's saved keyboards |
| `kc-zmk` | ZMK knowledge as data: keycodes, behaviour catalogue, Kconfig options |
| `kc-boards` | Board definitions: physical layouts, LED maps, firmware profiles |
| `kc-emit` | Generates `.keymap`, `.conf`, `west.yml` and `build.yaml` |
| `kc-import` | Imports existing `.keymap` files and MoErgo Layout Editor JSON |
| `kc-build` | Firmware build back ends (GitHub Actions first) |
| `kc-flash` | UF2 bootloader detection and flashing |
| `kc-device` | Finds connected keyboards over USB and recognises their board |
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

## Licence

MIT. See `LICENSE`.
