//! The Lighting mode: paint a color onto each key, layer by layer.

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use kc_boards::geometry::Spot;
use kc_model::features::{KeyLight, LockKind, Rgb};
use kc_model::keycap::{keycap, Keycap, KeycapKind};
use kc_model::lighting::{
    by_key_type, display_color, effective, effective_underglow, led_check, led_check_underglow,
};
use kc_zmk::Feature;

use super::{chip, heading, help, section_title, Workspace};
use crate::canvas::{self, Frame, Lit, Palette};

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

    /// Where the underglow lights are drawn, when the firmware paints them
    /// light by light; empty otherwise, and the colors stay in the layout.
    fn underglow_spots(&self) -> &[Spot] {
        if !self.features().contains(&Feature::UnderglowLighting) {
            return &[];
        }
        self.board
            .layout(&self.project().layout)
            .map_or(&[], |l| l.underglow.as_slice())
    }

    /// Whether the firmware shows per-key colors from power-up.
    fn starts_lit(&self) -> bool {
        self.board
            .profile(&self.config.profile)
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

    /// Paints an underglow light; the layer's list of them is made the
    /// layout's length first, the rest inheriting.
    fn paint_glow(&mut self, index: usize, cx: &mut Context<Self>) {
        let (layer, light) = (self.layer, self.brush_light());
        let count = self.underglow_spots().len();
        let _ = self.editor.edit("Paint Lighting", |p| {
            let underglow = &mut p.lighting_mut(layer)?.underglow;
            underglow.resize(count, KeyLight::Inherit);
            if let Some(glow) = underglow.get_mut(index) {
                *glow = light;
            }
            Ok(())
        });
        cx.notify();
    }

    fn paint_lit(&mut self, lit: Lit, cx: &mut Context<Self>) {
        match lit {
            Lit::Key(key) => self.paint_key(key, cx),
            Lit::Glow(index) => self.paint_glow(index, cx),
        }
    }

    /// Replaces the whole layer's lighting: the keys, and the underglow
    /// where the keyboard paints it (empty leaves every light inheriting).
    fn set_layer_lights(
        &mut self,
        label: &str,
        lights: Vec<KeyLight>,
        underglow: Vec<KeyLight>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let layer = self.layer;
        let spots = self.underglow_spots().len();
        self.change(label, window, cx, |p| {
            let lighting = p.lighting_mut(layer)?;
            if lights.len() == lighting.keys.len() {
                lighting.keys = lights;
            }
            if underglow.is_empty() || underglow.len() == spots {
                lighting.underglow = underglow;
            }
            Ok(())
        });
    }

    /// The layer's underglow lights as they are, for tools that leave
    /// them alone.
    fn current_underglow(&self) -> Vec<KeyLight> {
        self.project()
            .lighting(self.layer)
            .map(|l| l.underglow.clone())
            .unwrap_or_default()
    }

    /// The two LED check patterns for this layout: one color per row of
    /// the keymap, and one per column.
    fn led_check_patterns(&self) -> (Vec<KeyLight>, Vec<KeyLight>) {
        match self.board.layout(&self.project().layout) {
            Some(layout) => led_check(&layout.keys),
            None => {
                let keys = self.project().key_count;
                (vec![KeyLight::Inherit; keys], vec![KeyLight::Inherit; keys])
            }
        }
    }

    /// Whether every LED map of the board has been confirmed on hardware.
    fn leds_verified(&self) -> bool {
        self.board
            .halves
            .iter()
            .all(|half| half.leds.as_ref().is_none_or(|leds| leds.verified))
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
                    let lights = this
                        .project()
                        .lighting(id)
                        .map(|l| (l.keys.clone(), l.underglow.clone()));
                    if let Some((lights, underglow)) = lights {
                        this.set_layer_lights("Copy Lighting", lights, underglow, window, cx);
                    }
                }))
            })
            .collect::<Vec<_>>();

        div()
            .flex_none()
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
                    .child(section_title("COLOR", cx))
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
                    .child(brush("brush-color", "Color", Brush::Color, cx))
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
                            let light = this.brush_light();
                            let glow = vec![light; this.underglow_spots().len()];
                            this.set_layer_lights("Fill Lighting", vec![light; keys], glow, window, cx);
                        },
                    )))
                    .child(chip("lights-clear", "Clear", false, cx).on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.set_layer_lights("Clear Lighting", vec![KeyLight::Inherit; keys], Vec::new(), window, cx);
                        },
                    )))
                    .child(chip("lights-by-type", "Color by what keys do", false, cx).on_click(
                        cx.listener(|this, _, window, cx| {
                            let lights = by_key_type(this.project(), this.layer);
                            let glow = this.current_underglow();
                            this.set_layer_lights("Color by Key Type", lights, glow, window, cx);
                        }),
                    ))
                    .children(others),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(section_title("LED CHECK", cx))
                    .child(chip("lights-check-rows", "One color per row", false, cx).on_click(
                        cx.listener(|this, _, window, cx| {
                            let (rows, _) = this.led_check_patterns();
                            let glow = led_check_underglow(this.underglow_spots().len());
                            this.set_layer_lights("LED Check by Row", rows, glow, window, cx);
                        }),
                    ))
                    .child(chip("lights-check-columns", "One color per column", false, cx).on_click(
                        cx.listener(|this, _, window, cx| {
                            let (_, columns) = this.led_check_patterns();
                            let glow = led_check_underglow(this.underglow_spots().len());
                            this.set_layer_lights("LED Check by Column", columns, glow, window, cx);
                        }),
                    ))
                    .child(div().text_xs().text_color(muted).child(match (self.leds_verified(), self.underglow_spots().is_empty()) {
                        (true, true) => "For checking that each LED sits under the key the app thinks it does.",
                        (true, false) => "For checking that each LED sits under the key the app thinks it does, and that the underglow runs in the order drawn: its colors repeat along the strip.",
                        (false, _) => "This board's LED order has not been confirmed on hardware. Put these on the keyboard: a key lit in the wrong color is a key whose LED is mapped wrongly.",
                    })),
            )
            .child(help(format!(
                "Click or drag across keys{} to paint. Lock lights show the chosen color while the lock is on; battery lights turn red below the level. Colors are shown at full strength; the keyboard limits brightness to {}%.{}",
                if self.underglow_spots().is_empty() {
                    ""
                } else {
                    " and the underglow lights around them"
                },
                self.board.brightness_cap,
                if self.starts_lit() || self.config.delivery(&self.board) != kc_firmware::Delivery::Build {
                    ""
                } else {
                    " After flashing, press the key that changes the lighting effect until these colors appear; the keyboard remembers."
                }
            ), cx))
    }

    pub(super) fn render_lighting(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        // The Lighting tab is not offered without support, so this is only
        // seen if the firmware changes while the mode is showing.
        if !self.lighting_supported() {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(heading("Per-key lighting", cx))
                .child(div().max_w(px(560.)).text_sm().text_color(muted).child(
                    "This board's firmware has no per-key colors. The firmware is chosen on the board's page.",
                ));
        }

        let palette = Palette::plain(cx);
        let keys = self.layout_keys().to_vec();
        let blank = Keycap {
            legend: String::new(),
            hold: None,
            kind: KeycapKind::None,
            marks: Vec::new(),
        };
        let colors = (0..keys.len())
            .map(|position| {
                let (light, inherited) = effective(self.project(), self.layer, position);
                display_color(light).map(|c| to_hsla(c).opacity(if inherited { 0.4 } else { 1. }))
            })
            .collect();
        let glow = self
            .underglow_spots()
            .iter()
            .enumerate()
            .map(|(index, spot)| {
                let (light, inherited) = effective_underglow(self.project(), self.layer, index);
                let color = display_color(light)
                    .map(|c| to_hsla(c).opacity(if inherited { 0.4 } else { 1. }));
                (*spot, color)
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
            bands: Vec::new(),
            glow,
            hovered_glow: self.hovered_glow,
        };
        let bounds = self.canvas_bounds.clone();
        let lit_under = |this: &Self, position: Point<Pixels>| {
            canvas::lit_at(
                this.layout_keys(),
                this.underglow_spots(),
                this.canvas_bounds.get(),
                position,
            )
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
                            if let Some(lit) = lit_under(this, event.position) {
                                this.paint_lit(lit, cx);
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        let lit = lit_under(this, event.position);
                        let (hovered, hovered_glow) = match lit {
                            Some(Lit::Key(key)) => (Some(key), None),
                            Some(Lit::Glow(index)) => (None, Some(index)),
                            None => (None, None),
                        };
                        if hovered != this.hovered || hovered_glow != this.hovered_glow {
                            this.hovered = hovered;
                            this.hovered_glow = hovered_glow;
                            cx.notify();
                        }
                        if let (true, Some(lit)) = (this.painting, lit) {
                            this.paint_lit(lit, cx);
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
