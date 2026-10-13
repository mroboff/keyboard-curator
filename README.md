# Keyboard Curator

A native, open-source keymap, feature and per-key lighting editor for programmable keyboards: ZMK first, with RMK and Dygma alongside.

**Status: early development.** Keymap and behavior editing, per-key lighting, import, config generation, GitHub builds, guided flashing and direct updates over ZMK Studio are all built. The generated firmware configs are compiled in CI, but little has been tried on real keyboards yet.

The first release targets macOS and Linux and four boards: the Cyboard Imprint (82-key, wireless), the MoErgo Go60 and the MoErgo Glove80, on ZMK or RMK, and the Dygma Defy on Dygma's own firmware. More boards and Windows are planned afterward.

## Installing

Each release on GitHub carries a disk image for macOS and a tarball for Linux (x86_64).

**macOS**: open the `.dmg` and drag Keyboard Curator into Applications.

**Linux**: unpack `Keyboard-Curator-<version>-linux-x86_64.tar.gz`. It holds the `keyboard-curator` binary, a desktop entry and icon, the udev rules, the license files and a `README.txt` with the same steps as here:

```sh
tar -xzf Keyboard-Curator-*-linux-x86_64.tar.gz && cd keyboard-curator-*/
install -Dm755 keyboard-curator ~/.local/bin/keyboard-curator
install -Dm644 keyboard-curator.desktop ~/.local/share/applications/keyboard-curator.desktop
install -Dm644 keyboard-curator.png ~/.local/share/icons/hicolor/256x256/apps/keyboard-curator.png
sudo cp 70-keyboard-curator.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

The udev rules (`packaging/linux/70-keyboard-curator.rules`) give the logged-in user access to the keyboards' serial ports and raw HID interfaces; unplug and replug the keyboard after installing them. Without the rules, the serial port alone works for members of the `dialout` group (Debian, Ubuntu) or `uucp` (Arch), and writing a layout over HID (RMK, VIA) does not. The app needs Vulkan, Wayland or X11, fontconfig, ALSA and libudev, which every desktop distribution has. Keyboard shortcuts use Control on Linux where the guide says ⌘.

## Building

Requires the stable Rust toolchain (pinned in `rust-toolchain.toml`).

On macOS, the full Xcode app: the GUI framework compiles Metal shaders at build time, which the Command Line Tools alone cannot do. If `xcrun -f metal` fails, either run `sudo xcode-select -s /Applications/Xcode.app` or set `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` when building.

On Linux, the development packages the GUI framework, the key tester's sound and the USB crates link against: on Debian and Ubuntu, `pkg-config cmake clang libclang-dev libfontconfig1-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libxcb1-dev libvulkan-dev libasound2-dev libudev-dev libssl-dev` (the full list is in `.github/workflows/ci.yml`). On NixOS, `shell.nix` provides them: `nix-shell --run "cargo run -p kc-app"`.

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
- **A board's page** has five parts. *Firmware* chooses the firmware, and for ZMK builds one from parts: a base plus add-ons from the ZMK community, each described with what it is for. *Settings* adjusts that firmware's settings. *Layouts* creates, opens and imports layouts, and marks the current one. The fourth depends on the firmware: *Build & Flash* for firmware that is built, *Keyboard* for firmware configured on the keyboard itself. *Key Tester* is for trying the keyboard out.
- **The layout editor** edits one layout: keys, layers, behaviors, combos, per-key colors, pointing, layer rules. It shows only what the board's firmware supports. *Apply* makes the layout the board's current one.
- **Key Tester**, on a board's page, lights each key as you press it on the keyboard and keeps count of what has been seen, layer by layer and function by function: hold-taps, tap-dances, mod-morphs, combos, macros and mouse keys each confirm on their own. Presses are matched against the board's current layout, and the tester follows the keyboard to the layer it must be on. Every key also plays a note: choose a sine, triangle, sawtooth or square wave and a scale, and the board becomes an instrument. `cargo run -p kc-sound --example scale` checks the sound output by itself.
- **New Layout**, on a board's page, starts from the vendor's factory layout. A board with more templates offers a menu of them: the Imprint also has TailorKey, Moosy Research's layout for the MoErgo boards (https://sites.google.com/view/tailorkey) brought to the Imprint with its bilateral home-row mods, combos, macros and per-layer colors, the trackballs standing in for the touchpads. Community templates name their author.
- **Import a Keymap**, on a board's page, opens an existing `.keymap` file or a MoErgo Layout Editor export as a new layout. Firmware settings found with it are offered to the board.
- **Themes**: *View › Theme* chooses the app's look. Each is a data file, and the typefaces they use are carried in the app (all under the SIL Open Font License).
  - *Gallery*, the default: the keyboard set out like an exhibit, with sculpted keycaps on a plinth, large display type and one accent color.
  - *Arena*: near-black with violet and lime, capitals, and keys lit along their lower edge by kind.
  - *Curator*: the terminal look of vm-curator, in monospace, with the editor's shortcuts in its status line.
  - *Bench*: a workshop. Light keycaps with slate and orange accents on a dark desk mat, and a pegboard behind My Boards.
- **Help menu**: *Keyboard Shortcuts* lists every shortcut. *Check for Updates…* asks GitHub for the newest release and offers its page; the same check runs when the app starts unless *Check for Updates at Launch* is turned off. It sends nothing but the request.
- **LED check**: the Lighting mode has two patterns, one color per row and one per column, for confirming that each LED sits under the key the board definition says. On a board whose LED order has not been seen on hardware (the Imprint), put them on the keyboard and note any key lit in the wrong color.
- **Light and dark**: Gallery comes in both and follows the computer's appearance; Arena, Curator and Bench are dark. *View › Appearance* holds it to light or dark instead.

### Firmware families

| Family | Boards | How a layout reaches the keyboard |
|---|---|---|
| ZMK | Imprint, Go60, Glove80 | Generated config, built on GitHub, flashed as UF2. Key changes can also be sent over USB with ZMK Studio firmware. |
| RMK (experimental) | Imprint | Generated project, built on GitHub, flashed as UF2. Compiled, never flashed. |
| RMK for MoErgo (experimental) | Go60, Glove80, Imprint | colonelpanic's moergo-rmk, taken ready-made from its releases and checked against their checksums, flashed once as UF2. The layout is then written to the running keyboard over USB with Rynk, RMK's host protocol: keys, hold-taps with their timing profiles, tap-dances, mod-morphs, macros, combos, per-key colors with lock and battery lights, and the touchpads. Reads and writes TailorKey's RMK `.toml` files. The Imprint runs it from a fork with a board crate of its own (per-key RGB at the vendor's 50% ceiling, both PMW3610 trackballs, BLE split). Written against the protocol's own client; no keyboard has been connected yet. |
| Dygma | Defy | Written to the keyboard over USB, with no build. Reading is proven on a Defy; writing has only been run against a simulated keyboard. |
| Curator firmware (experimental) | Defy (wireless) | This project's fork of Dygma's 2.2.1 firmware, adding combos and a hold and tap feel of each key's own, configured over USB like the stock firmware. Installed with Bazecor's firmware update from the release's files; going back is the same update with Dygma's release. Not yet run on a keyboard. |

Building needs the GitHub CLI signed in (`gh auth login`) or a `GH_TOKEN`.

`fixtures/` holds a generated config for each board. CI builds them with the real ZMK toolchain; after an intended change to the emitter, refresh them with `UPDATE_FIXTURES=1 cargo test -p kc-emit --test fixtures`.

## Layout

A Cargo workspace under `crates/`. Only `kc-app` may depend on the GUI framework; CI enforces this with `ci/check-gui-boundary.sh`.

| Crate | Responsibility |
|---|---|
| `kc-model` | The layout: layers, bindings, behaviors, combos, macros, pointing, lighting; edit commands with undo/redo. Also the user's saved boards and their firmware settings |
| `kc-zmk` | ZMK knowledge as data: keycodes, behavior catalog, Kconfig options, and the add-on catalog |
| `kc-boards` | Board definitions: physical layouts, LED maps, firmware profiles |
| `kc-emit` | Generates `.keymap`, `.conf`, `west.yml` and `build.yaml` |
| `kc-rmk` | RMK keymaps and the project that builds them; layouts to and from moergo-rmk's runtime configuration |
| `kc-rynk` | Rynk, RMK's host protocol: reading and writing a keyboard's configuration, bootloader and reset |
| `kc-dygma` | Dygma's Focus protocol, and layouts to and from the keyboard |
| `kc-firmware` | One front door to the firmware families |
| `kc-import` | Imports existing `.keymap` files, MoErgo Layout Editor JSON and RMK runtime configuration files |
| `kc-build` | Firmware build back ends (GitHub Actions first), and firmware from GitHub releases |
| `kc-flash` | UF2 bootloader detection and flashing |
| `kc-device` | Finds connected keyboards over USB and recognizes their board |
| `kc-studio` | ZMK Studio transport and RPC for live editing |
| `kc-sound` | The key tester's synthesizer: scales, waveforms and sound output |
| `kc-app` | The desktop application |

## Contributing

Before sending a change, run what CI runs:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
ci/check-gui-boundary.sh
```

`CONTRIBUTING.md` explains how the crates fit together and how a board is added. `docs/packaging.md` covers the macOS and Linux release builds and `docs/release-checklist.md` the checks a release goes through on real keyboards. `docs/user-guide` holds the user guide; `docs/user-guide/build.py` prints it to PDF.

## License

MIT. See `LICENSE`. What the app carries from other projects, and under which terms, is listed in `THIRD-PARTY.md` (regenerate it with `ci/third-party.py`).
