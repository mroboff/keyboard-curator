# ZMK v0.3.0 with per-key lighting

A patch series that adds per-key, per-layer lighting (`zmk,underglow-layer`)
to upstream ZMK v0.3.0, for boards whose firmware is built from upstream ZMK
rather than a vendor fork. The Cyboard Imprint is the first.

Apply it to a clean checkout of the tag:

```sh
git clone https://github.com/zmkfirmware/zmk && cd zmk
git checkout -b per-key-lighting v0.3.0
git am /path/to/firmware/zmk-v0.3.0-per-key-lighting/*.patch
```

## Where the patches come from

| Patch | Source |
|---|---|
| 0001, 0004 | darknao's per-key lighting work for MoErgo's fork (https://github.com/moergo-sc/zmk/pull/36) |
| 0002, 0003 | upstream ZMK commits that work depends on (#3103, #3120) |
| 0005 to 0008 | darknao's upstream pull request (https://github.com/zmkfirmware/zmk/pull/2752) |
| 0009 | the `&trans` lighting commit from MoErgo PR 36, with two conflicts resolved against upstream's `rgb_underglow.c` |
| 0010 | ours: lets the keyboard start in the per-key effect |

All of it is MIT-licensed, like ZMK. Each patch keeps its original author.

## What it adds

- A `zmk,underglow-layer` node: one child per layer, each with a `bindings`
  list of one lighting behavior per key and a `layer-id`. The board supplies
  `pixel-lookup`, the key position under each LED.
- Lighting behaviors: `&ug <0xRRGGBB>`; `&ug_cl`, `&ug_nl` and `&ug_sl`
  `<off> <on>` for lock indicators; `&ug_b2`, `&ug_b4`, `&ug_b6` and `&ug_b8`
  `<below> <above>` for battery level; and `&trans` to show the layer below.
- The active layers are sent to the peripheral half so it can light too.
- `CONFIG_EXPERIMENTAL_RGB_LAYER=y` enables it. It appears as a fifth
  lighting effect; `CONFIG_ZMK_RGB_UNDERGLOW_EFF_START=4` with
  `CONFIG_ZMK_RGB_UNDERGLOW_ON_START=y` starts the keyboard in it.

## Status

The series applies cleanly and has been read through, but it has not been
compiled or run on hardware yet. Upstream ZMK intends to replace this
approach with a general lighting system, so treat it as a stopgap.
