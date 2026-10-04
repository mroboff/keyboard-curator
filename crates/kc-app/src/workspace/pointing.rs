//! The Pointing mode: what each trackball or touchpad does.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::features::{PointingConfig, PointingProfile};

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

impl Workspace {
    /// Sets what a device does. `None` returns it to the board's default.
    fn set_pointing(
        &mut self,
        listener: String,
        profile: Option<PointingProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change("Change Pointing Device", window, cx, |p| {
            let existing = p.pointing.iter().position(|d| d.listener == listener);
            match (profile, existing) {
                (None, Some(at)) => {
                    p.pointing.remove(at);
                }
                (None, None) => {}
                (Some(profile), Some(at)) => p.pointing[at].processors = profile.to_processors(),
                (Some(profile), None) => p.pointing.push(PointingConfig {
                    listener,
                    processors: profile.to_processors(),
                    overrides: Vec::new(),
                }),
            }
            Ok(())
        });
    }

    fn render_device(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);
        let device = &self.board.pointing[index];
        let listener = device.listener.clone();
        let config = self
            .project()
            .pointing
            .iter()
            .find(|d| d.listener == listener);
        let profile = config.map(|c| PointingProfile::from_processors(&c.processors));

        // A click handler that applies a change to the current profile.
        let with = |id: ElementId,
                    label: String,
                    active: bool,
                    cx: &mut Context<Self>,
                    edit: Box<dyn Fn(&mut PointingProfile)>| {
            let listener = listener.clone();
            let base = profile.flatten().unwrap_or_default();
            chip(id, label, active, cx).on_click(cx.listener(move |this, _, window, cx| {
                let mut profile = base;
                edit(&mut profile);
                this.set_pointing(listener.clone(), Some(profile), window, cx);
            }))
        };

        let default_listener = listener.clone();
        let card = div()
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
                            "Board default",
                            config.is_none(),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.set_pointing(default_listener.clone(), None, window, cx);
                            },
                        )),
                    )
                    .child(with(
                        ("pointing-move", index).into(),
                        "Move the pointer".into(),
                        matches!(profile, Some(Some(p)) if !p.scroll),
                        cx,
                        Box::new(|p| p.scroll = false),
                    ))
                    .child(with(
                        ("pointing-scroll", index).into(),
                        "Scroll".into(),
                        matches!(profile, Some(Some(p)) if p.scroll),
                        cx,
                        Box::new(|p| p.scroll = true),
                    )),
            );
        let current = match profile {
            None => {
                return card.child(div().text_sm().text_color(muted).child(
                    "Uses the behaviour built into the board's firmware. Choose an option to change it.",
                ));
            }
            Some(None) => {
                return card.child(div().text_sm().text_color(muted).child(
                    "This device has processing the editor does not model, so it is left as it is. Choosing an option above replaces it.",
                ));
            }
            Some(Some(profile)) => profile,
        };

        let speeds = SPEEDS
            .iter()
            .enumerate()
            .map(|(i, (label, speed))| {
                let speed = *speed;
                with(
                    ("pointing-speed", (index << 8) | i).into(),
                    (*label).into(),
                    current.speed == speed,
                    cx,
                    Box::new(move |p| p.speed = speed),
                )
            })
            .collect::<Vec<_>>();
        let mut auto = vec![with(
            ("pointing-auto-none", index).into(),
            "None".into(),
            current.auto_layer.is_none(),
            cx,
            Box::new(|p| p.auto_layer = None),
        )];
        for layer in &self.project().layers {
            let id = layer.id;
            let timeout = current.auto_layer.map_or(500, |(_, ms)| ms);
            auto.push(with(
                ("pointing-auto", (index << 16) | id.0 as usize).into(),
                layer.name.clone(),
                current.auto_layer.is_some_and(|(l, _)| l == id),
                cx,
                Box::new(move |p| p.auto_layer = Some((id, timeout))),
            ));
        }
        let step_listener = listener.clone();
        card.child(section_title("SPEED", cx))
            .child(div().flex().flex_wrap().gap_1().children(speeds))
            .child(section_title("DIRECTION", cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(with(
                        ("pointing-invert-x", index).into(),
                        "Reverse left and right".into(),
                        current.invert_x,
                        cx,
                        Box::new(|p| p.invert_x = !p.invert_x),
                    ))
                    .child(with(
                        ("pointing-invert-y", index).into(),
                        "Reverse up and down".into(),
                        current.invert_y,
                        cx,
                        Box::new(|p| p.invert_y = !p.invert_y),
                    ))
                    .child(with(
                        ("pointing-swap", index).into(),
                        "Swap the two axes".into(),
                        current.swap_xy,
                        cx,
                        Box::new(|p| p.swap_xy = !p.swap_xy),
                    )),
            )
            .child(section_title("LAYER TO SWITCH ON WHILE IN USE", cx))
            .child(div().flex().flex_wrap().gap_1().children(auto))
            .when_some(current.auto_layer, |card, (layer, timeout)| {
                card.child(self.stepper(
                    "pointing-timeout",
                    "Stay on it for",
                    format!("{timeout} ms after the last movement"),
                    cx,
                    move |this, d, window, cx| {
                        let mut profile = current;
                        let timeout = timeout.saturating_add_signed(d as i32 * 100).max(100);
                        profile.auto_layer = Some((layer, timeout));
                        this.set_pointing(step_listener.clone(), Some(profile), window, cx);
                    },
                ))
            })
    }

    pub(super) fn render_pointing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let cards = (0..self.board.pointing.len())
            .map(|index| self.render_device(index, cx))
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
