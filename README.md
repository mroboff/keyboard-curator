# keyboard-curator

A native, open-source keymap, feature and per-key lighting editor for ZMK keyboards.

**Status: early development.** Nothing is usable yet.

The first release targets macOS and two boards: the Cyboard Imprint (82-key, wireless) and the MoErgo Go60. More ZMK boards, Windows and Linux are planned afterwards.

## Building

Requires the stable Rust toolchain (pinned in `rust-toolchain.toml`) and, on macOS, the full Xcode app: the GUI framework compiles Metal shaders at build time, which the Command Line Tools alone cannot do. If `xcrun -f metal` fails, either run `sudo xcode-select -s /Applications/Xcode.app` or set `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` when building.

```sh
cargo build --workspace
cargo test --workspace
cargo run -p kc-app
```

## Layout

A Cargo workspace under `crates/`. Only `kc-app` may depend on the GUI framework; CI enforces this with `ci/check-gui-boundary.sh`.

| Crate | Responsibility |
|---|---|
| `kc-model` | The project: layers, bindings, behaviours, combos, macros, pointing, lighting, settings; edit commands with undo/redo |
| `kc-zmk` | ZMK knowledge as data: keycodes, behaviour catalogue, Kconfig options |
| `kc-boards` | Board definitions: physical layouts, LED maps, firmware profiles |
| `kc-emit` | Generates `.keymap`, `.conf`, `west.yml` and `build.yaml` |
| `kc-import` | Imports existing `.keymap` files and MoErgo Layout Editor JSON |
| `kc-build` | Firmware build back ends (GitHub Actions first) |
| `kc-flash` | UF2 bootloader detection and flashing |
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
