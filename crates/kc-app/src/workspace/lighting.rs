//! The Lighting mode: paint a colour onto each key, layer by layer.

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use kc_model::features::{KeyLight, LockKind, Rgb};
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::lighting::{by_key_type, display_color, effective};
use kc_zmk::Feature;

use super::{chip, section_title, Workspace};
use crate::canvas::{self, Frame, Palette};

/// What painting a key gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Brush {
    Color,
    Off,
    Inherit,
    Lock(LockKind),
    Battery(u8),
}

/// The swatches always on offer.
const SWATCHES: [Rgb; 12] = [
    Rgb(0xFF, 0x00, 0x00),
    Rgb(0xFF, 0x80, 0x00),
    Rgb(0xFF, 0xD7, 0x00),
    Rgb(0x80, 0xFF, 0x00),
    Rgb(0x00, 0xFF, 0x00),
    Rgb(0x00, 0xFF, 0xA0),
    Rgb(0x00, 0xC0, 0xFF),
    Rgb(0x00, 0x40, 0xFF),
    Rgb(0x80, 0x00, 0xFF),
    Rgb(0xFF, 0x00, 0xC0),
    Rgb(0xFF, 0xC0, 0xCB),
    Rgb(0xFF, 0xFF, 0xFF),
];

pub(super) fn to_hsla(color: Rgb) -> Hsla {
    rgb(u32::from(color.0) << 16 | u32::from(color.1) << 8 | u32::from(color.2)).into()
}

fn to_rgb(color: Hsla) -> Rgb {
    let rgba = Rgba::from(color);
    let channel = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    Rgb(channel(rgba.r), channel(rgba.g), channel(rgba.b))
}

pub(super) fn subscribe(picker: &Entity<ColorPickerState>, cx: &mut Context<Workspace>) {
    cx.subscribe(picker, |this, _, event: &ColorPickerEvent, cx| {
        let ColorPickerEvent::Change(Some(color)) = event else {
            return;
        };
        this.paint_color = to_rgb(*color);
        if this.brush == Brush::Off || this.brush == Brush::Inherit {
            this.brush = Brush::Color;
        }
        cx.notify();
    })
    .detach();
}

impl Workspace {
    fn lighting_supported(&self) -> bool {
        self.features().contains(&Feature::PerKeyLighting)
    }

    /// Whether the firmware shows per-key colours from power-up.
    fn starts_lit(&self) -> bool {
        self.board
            .profile(&self.project().firmware)
            .and_then(|p| p.lighting.as_ref())
            .is_some_and(|l| l.start_effect.is_some())
    }

    /// The light the current brush paints.
    fn brush_light(&self) -> KeyLight {
        let color = self.paint_color;
        match self.brush {
            Brush::Color => KeyLight::Color(color),
            Brush::Off => KeyLight::Off,
            Brush::Inherit => KeyLight::Inherit,
            Brush::Lock(lock) => KeyLight::Lock {
                lock,
                off: Rgb(0, 0, 0),
                on: color,
            },
            Brush::Battery(percent) => KeyLight::Battery {
                percent,
                below: Rgb(0xFF, 0, 0),
                above: color,
            },
        }
    }

    fn paint_key(&mut self, position: usize, cx: &mut Context<Self>) {
        let (layer, light) = (self.layer, self.brush_light());
        let _ = self.editor.edit("Paint Lighting", |p| {
            if let Some(key) = p.lighting_mut(layer)?.keys.get_mut(position) {
                *key = light;
            }
            Ok(())
        });
        cx.notify();
    }

    /// Replaces the whole layer's lighting.
    fn set_layer_lights(
        &mut self,
        label: &str,
        lights: Vec<KeyLight>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let layer = self.layer;
        self.change(label, window, cx, |p| {
            let lighting = p.lighting_mut(layer)?;
            if lights.len() == lighting.keys.len() {
                lighting.keys = lights;
            }
            Ok(())
        });
    }

    fn render_lighting_tools(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (border, muted, foreground) = (theme.border, theme.muted_foreground, theme.foreground);
        let brush = |id: &'static str,
                     label: &'static str,
                     brush: Brush,
                     cx: &mut Context<Self>| {
            chip(id, label, self.brush == brush, cx).on_click(cx.listener(move |this, _, _, cx| {
                this.brush = brush;
                cx.notify();
            }))
        };
        let swatches = SWATCHES
            .into_iter()
            .enumerate()
            .map(|(index, color)| {
                let active = self.paint_color == color;
                div()
                    .id(("swatch", index))
                    .size_7()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(to_hsla(color))
                    .border_2()
                    .border_color(if active { foreground } else { border })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.paint_color = color;
                        if matches!(this.brush, Brush::Off | Brush::Inherit) {
                            this.brush = Brush::Color;
                        }
                        cx.notify();
                    }))
            })
            .collect::<Vec<_>>();
        let keys = self.project().key_count;
        let others = self
            .project()
            .layers
            .iter()
            .filter(|l| l.id != self.layer && self.project().lighting(l.id).is_some())
            .map(|l| {
                let id = l.id;
                chip(
                    ("copy-lights", id.0 as usize),
                    format!("Copy from {}", l.name),
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    let lights = this.project().lighting(id).map(|l| l.keys.clone());
                    if let Some(lights) = lights {
                        this.set_layer_lights("Copy Lighting", lights, window, cx);
                    }
                }))
            })
            .collect::<Vec<_>>();

        div()
            .h_56()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .border_t_1()
            .border_color(border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(section_title("COLOUR", cx))
                    .children(swatches)
                    .child(ColorPicker::new(&self.color_picker))
                    .child(
                        div()
                            .size_7()
                            .rounded_md()
                            .border_1()
                            .border_color(border)
                            .bg(to_hsla(self.paint_color)),
                    )
                    .child(div().text_xs().text_color(muted).child(self.paint_color.to_string())),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(section_title("PAINT WITH", cx))
                    .child(brush("brush-color", "Colour", Brush::Color, cx))
                    .child(brush("brush-off", "Off", Brush::Off, cx))
                    .child(brush("brush-inherit", "Same as layer below", Brush::Inherit, cx))
                    .child(brush("brush-caps", "Caps Lock light", Brush::Lock(LockKind::Caps), cx))
                    .child(brush("brush-num", "Num Lock light", Brush::Lock(LockKind::Num), cx))
                    .child(brush("brush-scroll", "Scroll Lock light", Brush::Lock(LockKind::Scroll), cx))
                    .child(brush("brush-b20", "Battery above 20%", Brush::Battery(20), cx))
                    .child(brush("brush-b40", "40%", Brush::Battery(40), cx))
                    .child(brush("brush-b60", "60%", Brush::Battery(60), cx))
                    .child(brush("brush-b80", "80%", Brush::Battery(80), cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(section_title("WHOLE LAYER", cx))
                    .child(chip("lights-fill", "Fill", false, cx).on_click(cx.listener(
                        move |this, _, window, cx| {
                            let lights = vec![this.brush_light(); keys];
                            this.set_layer_lights("Fill Lighting", lights, window, cx);
                        },
                    )))
                    .child(chip("lights-clear", "Clear", false, cx).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.set_layer_lights("Clear Lighting", vec![KeyLight::Inherit; keys], window, cx);
                        },
                    )))
                    .child(chip("lights-by-type", "Colour by what keys do", false, cx).on_click(
                        cx.listener(|this, _, window, cx| {
                            let lights = by_key_type(this.project(), this.layer);
                            this.set_layer_lights("Colour by Key Type", lights, window, cx);
                        }),
                    ))
                    .children(others),
            )
            .child(div().text_xs().text_color(muted).child(format!(
                "Click or drag across keys to paint. Lock lights show the chosen colour while the lock is on; battery lights turn red below the level. Colours are shown at full strength; the keyboard limits brightness to {}%.{}",
                self.board.brightness_cap,
                if self.starts_lit() {
                    ""
                } else {
                    " After flashing, press the key that changes the lighting effect until these colours appear; the keyboard remembers."
                }
            )))
    }

    pub(super) fn render_lighting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        if !self.lighting_supported() {
            let alternatives = self
                .board
                .firmware
                .iter()
                .filter(|f| f.lighting.is_some())
                .map(|f| {
                    let id = f.id.clone();
                    chip(
                        ("switch-firmware", id.len()),
                        format!("Switch to {}", f.name),
                        false,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let id = id.clone();
                        this.change("Change Firmware", window, cx, |p| {
                            p.firmware = id;
                            Ok(())
                        });
                    }))
                })
                .collect::<Vec<_>>();
            let explanation = if alternatives.is_empty() {
                "Per-key colours need firmware support that is not available for this keyboard yet."
            } else {
                "Per-key colours need a community firmware that is still experimental. Switching changes which firmware your layout is built with; you can switch back in Settings."
            };
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(div().text_lg().child("Per-key lighting"))
                .child(
                    div()
                        .max_w(px(560.))
                        .text_sm()
                        .text_color(muted)
                        .child(explanation),
                )
                .children(alternatives);
        }

        let palette = Palette {
            key: theme.secondary,
            key_border: theme.border,
            text: theme.foreground,
            muted_text: theme.muted_foreground,
            accent: theme.primary,
            layer_key: theme.secondary,
        };
        let keys = self.layout_keys().to_vec();
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
        };
        let colors = (0..keys.len())
            .map(|position| {
                let (light, inherited) = effective(self.project(), self.layer, position);
                display_color(light).map(|c| to_hsla(c).opacity(if inherited { 0.4 } else { 1. }))
            })
            .collect();
        let frame = Frame {
            keycaps: (0..keys.len())
                .map(|p| keycap(self.project(), self.layer, p).unwrap_or(blank.clone()))
                .collect(),
            keys,
            devices: Vec::new(),
            selected: Vec::new(),
            hovered: self.hovered,
            drag: None,
            band: None,
            palette,
            tint: None,
            links: Vec::new(),
            colors,
        };
        let bounds = self.canvas_bounds.clone();
        let key_under = |this: &Self, position: Point<Pixels>| {
            canvas::key_at(this.layout_keys(), &[], this.canvas_bounds.get(), position)
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(canvas::keyboard(frame, move |b| bounds.set(b)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            // One undo step for the whole stroke.
                            this.editor.begin_group("Paint Lighting");
                            this.painting = true;
                            if let Some(key) = key_under(this, event.position) {
                                this.paint_key(key, cx);
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        let hovered = key_under(this, event.position);
                        if hovered != this.hovered {
                            this.hovered = hovered;
                            cx.notify();
                        }
                        if let (true, Some(key)) = (this.painting, hovered) {
                            this.paint_key(key, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.painting = false;
                            this.editor.end_group();
                            cx.notify();
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.painting = false;
                            this.editor.end_group();
                            cx.notify();
                        }),
                    ),
            )
            .child(self.render_lighting_tools(cx))
    }
}
