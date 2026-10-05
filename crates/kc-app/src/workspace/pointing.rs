//! The Pointing mode: what each trackball, touchpad and set of mouse keys
//! does, and what it does differently while particular layers are on.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::features::{InputProcessor, PointingConfig, PointingOverride, PointingProfile};
use kc_model::LayerId;
use kc_zmk::Feature;

use super::{chip, section_title, Workspace};

/// Speeds offered, as a label and a `multiplier / divisor` scale.
const SPEEDS: [(&str, (u32, u32)); 7] = [
    ("25%", (1, 4)),
    ("33%", (1, 3)),
    ("50%", (1, 2)),
    ("100%", (1, 1)),
    ("150%", (3, 2)),
    ("200%", (2, 1)),
    ("300%", (3, 1)),
];

/// Something the Pointing mode has a card for.
#[derive(Clone)]
struct Device {
    name: String,
    listener: String,
    /// The mouse keys, rather than a device on the board.
    mouse_keys: bool,
    /// Whether it reports scrolling itself.
    native_scroll: bool,
}

/// Which of a device's settings a control changes.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    /// The device's own.
    Device,
    /// Those for particular layers, by position in its list.
    Layers(usize),
}

/// A speed as a whole percentage, for showing.
fn percent((multiplier, divisor): (u32, u32)) -> u32 {
    let divisor = u64::from(divisor.max(1));
    ((u64::from(multiplier) * 100 + divisor / 2) / divisor) as u32
}

/// The speed five percentage points along from `speed`, as a fraction in
/// lowest terms.
fn step_speed(speed: (u32, u32), direction: i64) -> (u32, u32) {
    let twentieths = (i64::from(percent(speed)) / 5 + direction).clamp(1, 400) as u32;
    let gcd = |mut a: u32, mut b: u32| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let common = gcd(twentieths, 20);
    (twentieths / common, 20 / common)
}

impl Workspace {
    /// The board's pointing devices, then the mouse keys when the firmware
    /// has them.
    fn pointing_devices(&self) -> Vec<Device> {
        let mut devices: Vec<Device> = self
            .board
            .pointing
            .iter()
            .map(|d| Device {
                name: d.name.clone(),
                listener: d.listener.clone(),
                mouse_keys: false,
                native_scroll: false,
            })
            .collect();
        if self.features().contains(&Feature::Pointing) {
            for (listener, name, native_scroll) in kc_zmk::pointing::MOUSE_KEY_LISTENERS {
                devices.push(Device {
                    name: name.to_string(),
                    listener: listener.to_string(),
                    mouse_keys: true,
                    native_scroll,
                });
            }
        }
        devices
    }

    /// Sets the processors `target` has. `None` returns a device to the
    /// firmware's default, or removes the settings for particular layers.
    fn set_pointing(
        &mut self,
        listener: String,
        target: Target,
        processors: Option<Vec<InputProcessor>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change("Change Pointing Device", window, cx, |p| {
            let existing = p.pointing.iter().position(|d| d.listener == listener);
            match (target, existing) {
                (Target::Device, Some(at)) => {
                    p.pointing[at].processors = processors.unwrap_or_default();
                }
                (Target::Device, None) => {
                    if let Some(processors) = processors {
                        p.pointing.push(PointingConfig {
                            listener,
                            processors,
                            overrides: Vec::new(),
                        });
                    }
                }
                (Target::Layers(index), Some(at)) => {
                    let overrides = &mut p.pointing[at].overrides;
                    match processors {
                        Some(processors) if index < overrides.len() => {
                            overrides[index].processors = processors;
                        }
                        None if index < overrides.len() => {
                            overrides.remove(index);
                        }
                        _ => {}
                    }
                }
                (Target::Layers(_), None) => {}
            }
            // A device with nothing set is one left at its default.
            p.pointing
                .retain(|d| !(d.processors.is_empty() && d.overrides.is_empty()));
            Ok(())
        });
    }

    /// Starts settings for a device while `layer` is on, copying the
    /// device's own so that there is something to adjust.
    fn add_pointing_layer(
        &mut self,
        listener: String,
        layer: LayerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change("Add Pointing Layer Settings", window, cx, |p| {
            let at = match p.pointing.iter().position(|d| d.listener == listener) {
                Some(at) => at,
                None => {
                    p.pointing.push(PointingConfig {
                        listener,
                        processors: Vec::new(),
                        overrides: Vec::new(),
                    });
                    p.pointing.len() - 1
                }
            };
            // The layer a device switches on is the device's own business.
            let processors = p.pointing[at]
                .processors
                .iter()
                .filter(|p| !matches!(p, InputProcessor::TempLayer { .. }))
                .cloned()
                .collect();
            p.pointing[at].overrides.push(PointingOverride {
                layers: vec![layer],
                processors,
            });
            Ok(())
        });
    }

    /// The controls for one set of processors. `key` is unique to the
    /// device and target, for element IDs.
    fn render_pointing_controls(
        &self,
        key: usize,
        device: &Device,
        target: Target,
        processors: &[InputProcessor],
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let native_scroll = device.native_scroll;
        let column = div().flex().flex_col().gap_2();
        let Some(current) = PointingProfile::from_processors(processors, native_scroll) else {
            return column.child(div().text_sm().text_color(muted).child(
                "This has processing the editor does not model, so it is left as it is. Choosing an option above replaces it.",
            ));
        };

        // A chip that applies a change to the current settings.
        let listener = device.listener.clone();
        let with = |id: ElementId,
                    label: String,
                    active: bool,
                    cx: &mut Context<Self>,
                    edit: Box<dyn Fn(&mut PointingProfile)>| {
            let listener = listener.clone();
            chip(id, label, active, cx).on_click(cx.listener(move |this, _, window, cx| {
                let mut profile = current;
                edit(&mut profile);
                let processors = profile.to_processors(native_scroll);
                this.set_pointing(listener.clone(), target, Some(processors), window, cx);
            }))
        };

        let mut speeds = SPEEDS
            .iter()
            .enumerate()
            .map(|(i, (label, speed))| {
                let speed = *speed;
                with(
                    ("pointing-speed", (key << 8) | i).into(),
                    (*label).into(),
                    current.speed == speed,
                    cx,
                    Box::new(move |p| p.speed = speed),
                )
            })
            .collect::<Vec<_>>();
        // A speed from a file need not be one of those offered.
        if !SPEEDS.iter().any(|(_, speed)| *speed == current.speed) {
            let (multiplier, divisor) = current.speed;
            speeds.push(chip(
                ("pointing-speed-exact", key),
                format!("{}% ({multiplier}/{divisor})", percent(current.speed)),
                true,
                cx,
            ));
        }
        for (i, (label, direction)) in [("−", -1), ("+", 1)].into_iter().enumerate() {
            speeds.push(with(
                ("pointing-speed-step", (key << 8) | i).into(),
                label.into(),
                false,
                cx,
                Box::new(move |p| p.speed = step_speed(p.speed, direction)),
            ));
        }

        let mut column = column;
        if !device.mouse_keys {
            column = column.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(with(
                        ("pointing-move", key).into(),
                        "Move the pointer".into(),
                        !current.scroll,
                        cx,
                        Box::new(|p| p.scroll = false),
                    ))
                    .child(with(
                        ("pointing-scroll", key).into(),
                        "Scroll".into(),
                        current.scroll,
                        cx,
                        Box::new(|p| p.scroll = true),
                    )),
            );
        }
        column = column
            .child(section_title("SPEED", cx))
            .child(div().flex().flex_wrap().gap_1().children(speeds))
            .child(section_title("DIRECTION", cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(with(
                        ("pointing-invert-x", key).into(),
                        "Reverse left and right".into(),
                        current.invert_x,
                        cx,
                        Box::new(|p| p.invert_x = !p.invert_x),
                    ))
                    .child(with(
                        ("pointing-invert-y", key).into(),
                        "Reverse up and down".into(),
                        current.invert_y,
                        cx,
                        Box::new(|p| p.invert_y = !p.invert_y),
                    ))
                    .child(with(
                        ("pointing-swap", key).into(),
                        "Swap the two axes".into(),
                        current.swap_xy,
                        cx,
                        Box::new(|p| p.swap_xy = !p.swap_xy),
                    )),
            );
        if !device.mouse_keys {
            column = column.child(section_title("CLICK", cx)).child(
                div().flex().flex_wrap().gap_1().child(with(
                    ("pointing-right-click", key).into(),
                    "A click is a right click".into(),
                    current.right_click,
                    cx,
                    Box::new(|p| p.right_click = !p.right_click),
                )),
            );
        }
        // Which layer a device switches on is not something that changes
        // with the layer.
        if target != Target::Device && current.auto_layer.is_none() {
            return column;
        }

        let mut auto = vec![with(
            ("pointing-auto-none", key).into(),
            "None".into(),
            current.auto_layer.is_none(),
            cx,
            Box::new(|p| p.auto_layer = None),
        )];
        for layer in &self.project().layers {
            let id = layer.id;
            let timeout = current.auto_layer.map_or(500, |(_, ms)| ms);
            auto.push(with(
                ("pointing-auto", (key << 16) | id.0 as usize).into(),
                layer.name.clone(),
                current.auto_layer.is_some_and(|(l, _)| l == id),
                cx,
                Box::new(move |p| p.auto_layer = Some((id, timeout))),
            ));
        }
        column = column
            .child(section_title("LAYER TO SWITCH ON WHILE IN USE", cx))
            .child(div().flex().flex_wrap().gap_1().children(auto));
        if let Some((_, timeout)) = current.auto_layer {
            let steps = [("−", -100i32), ("+", 100)]
                .into_iter()
                .enumerate()
                .map(|(i, (label, change))| {
                    with(
                        ("pointing-timeout", (key << 8) | i).into(),
                        label.into(),
                        false,
                        cx,
                        Box::new(move |p| {
                            if let Some((_, ms)) = &mut p.auto_layer {
                                *ms = ms.saturating_add_signed(change).max(100);
                            }
                        }),
                    )
                })
                .collect::<Vec<_>>();
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().flex_1().text_sm().child(format!(
                        "Stay on it for: {timeout} ms after the last movement"
                    )))
                    .children(steps),
            );
        }
        column
    }

    fn render_device(&self, index: usize, device: &Device, cx: &mut Context<Self>) -> Div {
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);
        let listener = device.listener.clone();
        let config = self
            .project()
            .pointing
            .iter()
            .find(|d| d.listener == listener);
        let own: &[InputProcessor] = config.map_or(&[], |c| c.processors.as_slice());
        let overrides: &[PointingOverride] = config.map_or(&[], |c| c.overrides.as_slice());
        // Element IDs: 64 targets per device.
        let key = |target: usize| index * 64 + target;

        let (default_listener, custom_listener) = (listener.clone(), listener.clone());
        let mut card = div()
            .w(px(680.))
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .child(div().text_lg().child(device.name.clone()))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(
                        chip(
                            ("pointing-default", index),
                            "Firmware default",
                            own.is_empty(),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.set_pointing(
                                    default_listener.clone(),
                                    Target::Device,
                                    None,
                                    window,
                                    cx,
                                );
                            },
                        )),
                    )
                    .when(
                        PointingProfile::from_processors(own, device.native_scroll).is_none(),
                        |row| {
                            // The way out of processing the editor cannot show.
                            row.child(
                                chip(("pointing-replace", index), "Replace", false, cx).on_click(
                                    cx.listener(move |this, _, window, cx| {
                                        this.set_pointing(
                                            custom_listener.clone(),
                                            Target::Device,
                                            Some(Vec::new()),
                                            window,
                                            cx,
                                        );
                                    }),
                                ),
                            )
                        },
                    ),
            );
        if own.is_empty() {
            card = card.child(div().text_sm().text_color(muted).child(if device.mouse_keys {
                "Uses the speed and direction built into the firmware. Choose an option to change it."
            } else {
                "Uses the behaviour built into the board's firmware. Choose an option to change it."
            }));
        }
        card = card.child(self.render_pointing_controls(key(0), device, Target::Device, own, cx));

        card = card.child(section_title("WHILE PARTICULAR LAYERS ARE ON", cx));
        if !overrides.is_empty() {
            card = card.child(div().text_sm().text_color(muted).child(
                "While one of these layers is on, its settings are used in place of the ones above.",
            ));
        }
        for (at, layer_override) in overrides.iter().enumerate() {
            let names: Vec<String> = layer_override
                .layers
                .iter()
                .filter_map(|id| self.project().layer(*id))
                .map(|l| l.name.clone())
                .collect();
            let remove_listener = listener.clone();
            card = card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().flex_1().child(names.join(", ")))
                            .child(
                                chip(("pointing-layers-remove", key(at + 1)), "Remove", false, cx)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_pointing(
                                            remove_listener.clone(),
                                            Target::Layers(at),
                                            None,
                                            window,
                                            cx,
                                        );
                                    })),
                            ),
                    )
                    .when(layer_override.processors.is_empty(), |section| {
                        section.child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child("Nothing is changed: movement is passed on as it is."),
                        )
                    })
                    .child(self.render_pointing_controls(
                        key(at + 1),
                        device,
                        Target::Layers(at),
                        &layer_override.processors,
                        cx,
                    )),
            );
        }
        let add = self
            .project()
            .layers
            .iter()
            .filter(|l| !overrides.iter().any(|o| o.layers.contains(&l.id)))
            .map(|layer| {
                let (id, listener) = (layer.id, listener.clone());
                chip(
                    ("pointing-layers-add", (index << 16) | id.0 as usize),
                    layer.name.clone(),
                    false,
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.add_pointing_layer(listener.clone(), id, window, cx);
                }))
            })
            .collect::<Vec<_>>();
        card.child(
            div()
                .text_sm()
                .text_color(muted)
                .child("Add different settings for a layer:"),
        )
        .child(div().flex().flex_wrap().gap_1().children(add))
    }

    pub(super) fn render_pointing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let cards = self
            .pointing_devices()
            .iter()
            .enumerate()
            .map(|(index, device)| self.render_device(index, device, cx))
            .collect::<Vec<_>>();
        div()
            .id("pointing")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .gap_4()
            .when(cards.is_empty(), |page| {
                page.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("This keyboard has no trackball or touchpad."),
                )
            })
            .children(cards)
    }
}

#[cfg(test)]
mod tests {
    use super::{percent, step_speed};

    #[test]
    fn speeds_are_shown_and_stepped_as_percentages() {
        assert_eq!(percent((11, 12)), 92);
        assert_eq!(percent((1, 3)), 33);
        assert_eq!(percent((13, 3)), 433);
        assert_eq!(step_speed((1, 1), 1), (21, 20));
        assert_eq!(step_speed((1, 1), -1), (19, 20));
        assert_eq!(step_speed((11, 12), 1), (19, 20));
        assert_eq!(step_speed((11, 12), -1), (17, 20));
        assert_eq!(step_speed((1, 9), -1), (1, 20));
        assert_eq!(step_speed((1, 20), -1), (1, 20));
        assert_eq!(step_speed((1, 2), 1), (11, 20));
    }
}
