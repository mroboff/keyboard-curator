//! The Settings mode: firmware settings, layer rules and raw text.

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::features::{ConditionalLayer, SettingValue};
use kc_zmk::settings::{Setting, SettingKind, BRIGHTNESS_SETTINGS, SETTINGS};

use super::{chip, section_title, Workspace};

const KEYBOARD_NAME: &str = "CONFIG_ZMK_KEYBOARD_NAME";

pub(super) fn subscribe(
    keyboard_name: &Entity<InputState>,
    raw_behaviors: &Entity<TextareaState>,
    raw_devicetree: &Entity<TextareaState>,
    raw_conf: &Entity<TextareaState>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // Text fields are committed when focus leaves them or Return is pressed.
    let commit = |this: &mut Workspace,
                  event: &InputEvent,
                  window: &mut Window,
                  cx: &mut Context<Workspace>| {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            this.after_change(window, cx);
        }
    };
    cx.subscribe_in(
        keyboard_name,
        window,
        move |this, _, event: &InputEvent, window, cx| {
            commit(this, event, window, cx);
        },
    )
    .detach();
    for area in [raw_behaviors, raw_devicetree, raw_conf] {
        cx.subscribe_in(area, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::Blur) {
                this.after_change(window, cx);
            }
        })
        .detach();
    }
}

impl Workspace {
    /// Applies what has been typed into the settings text fields.
    pub(super) fn commit_settings_inputs(&mut self, cx: &mut Context<Self>) {
        let name = self.keyboard_name.read(cx).value().trim().to_string();
        let stored = match self.project().settings.get(KEYBOARD_NAME) {
            Some(SettingValue::Text(text)) => text.clone(),
            _ => String::new(),
        };
        let behaviors = self.raw_behaviors.read(cx).value().to_string();
        let devicetree = self.raw_devicetree.read(cx).value().to_string();
        let conf = self.raw_conf.read(cx).value().to_string();
        let raw = &self.project().raw;
        if name == stored
            && behaviors == raw.behaviors
            && devicetree == raw.devicetree
            && conf == raw.conf
        {
            return;
        }
        let _ = self.editor.edit("Change Settings", |p| {
            if name.is_empty() {
                p.settings.remove(KEYBOARD_NAME);
            } else {
                p.settings
                    .insert(KEYBOARD_NAME.into(), SettingValue::Text(name));
            }
            p.raw.behaviors = behaviors;
            p.raw.devicetree = devicetree;
            p.raw.conf = conf;
            Ok(())
        });
    }

    pub(super) fn sync_settings_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = match self.project().settings.get(KEYBOARD_NAME) {
            Some(SettingValue::Text(text)) => text.clone(),
            _ => String::new(),
        };
        if self.keyboard_name.read(cx).value().trim() != name {
            self.keyboard_name
                .update(cx, |input, cx| input.set_value(name, window, cx));
        }
        let raw = self.project().raw.clone();
        for (area, text) in [
            (&self.raw_behaviors, raw.behaviors),
            (&self.raw_devicetree, raw.devicetree),
            (&self.raw_conf, raw.conf),
        ] {
            if area.read(cx).value() != text {
                area.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
    }

    fn set_setting(
        &mut self,
        key: &'static str,
        value: Option<SettingValue>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.change("Change Setting", window, cx, |p| {
            match value {
                Some(value) => {
                    p.settings.insert(key.to_string(), value);
                }
                None => {
                    p.settings.remove(key);
                }
            }
            Ok(())
        });
    }

    fn render_setting(&self, setting: &'static Setting, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let key = setting.key;
        let value = self.project().settings.get(key).cloned();
        let choose = |id: &'static str,
                      label: String,
                      active: bool,
                      value: Option<SettingValue>,
                      cx: &mut Context<Self>| {
            chip(
                (
                    id,
                    key.len() * 131 + key.bytes().map(usize::from).sum::<usize>(),
                ),
                label,
                active,
                cx,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_setting(key, value.clone(), window, cx);
            }))
        };
        let controls = match setting.kind {
            SettingKind::Bool => div()
                .flex()
                .gap_1()
                .child(choose(
                    "setting-default",
                    "Board default".into(),
                    value.is_none(),
                    None,
                    cx,
                ))
                .child(choose(
                    "setting-on",
                    "On".into(),
                    value == Some(SettingValue::Bool(true)),
                    Some(SettingValue::Bool(true)),
                    cx,
                ))
                .child(choose(
                    "setting-off",
                    "Off".into(),
                    value == Some(SettingValue::Bool(false)),
                    Some(SettingValue::Bool(false)),
                    cx,
                )),
            SettingKind::Int { min, max, step } => {
                // Brightness settings stop at what the board allows.
                let max = if BRIGHTNESS_SETTINGS.contains(&key) {
                    max.min(i64::from(self.board.brightness_cap))
                } else {
                    max
                };
                let current = match value {
                    Some(SettingValue::Int(n)) => Some(n),
                    _ => None,
                };
                let shown = current.map_or_else(|| "Board default".to_string(), |n| n.to_string());
                let stepped = |delta: i64| {
                    Some(SettingValue::Int(match current {
                        Some(n) => (n + delta).clamp(min, max),
                        None if delta > 0 => min,
                        None => max,
                    }))
                };
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(div().w_32().text_sm().child(shown))
                    .child(choose(
                        "setting-less",
                        "−".into(),
                        false,
                        stepped(-step),
                        cx,
                    ))
                    .child(choose("setting-more", "+".into(), false, stepped(step), cx))
                    .when(current.is_some(), |row| {
                        row.child(choose(
                            "setting-default",
                            "Board default".into(),
                            false,
                            None,
                            cx,
                        ))
                    })
            }
            SettingKind::Text { .. } => div().w_64().child(Input::new(&self.keyboard_name)),
        };
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().text_sm().child(setting.name))
                    .child(div().text_xs().text_color(muted).child(setting.description)),
            )
            .child(controls)
    }

    fn edit_rule(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut ConditionalLayer),
    ) {
        self.change("Change Layer Rule", window, cx, |p| {
            if let Some(rule) = p.conditional_layers.get_mut(index) {
                edit(rule);
            }
            Ok(())
        });
    }

    fn render_rules(&self, cx: &mut Context<Self>) -> Div {
        let (border, muted) = (cx.theme().border, cx.theme().muted_foreground);
        let layers: Vec<_> = self
            .project()
            .layers
            .iter()
            .map(|l| (l.id, l.name.clone()))
            .collect();
        let rules = self
            .project()
            .conditional_layers
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let when = layers
                    .iter()
                    .map(|(id, name)| {
                        let id = *id;
                        chip(
                            ("rule-if", (index << 16) | id.0 as usize),
                            name.clone(),
                            rule.if_layers.contains(&id),
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.edit_rule(index, window, cx, |rule| {
                                    match rule.if_layers.iter().position(|l| *l == id) {
                                        Some(at) => {
                                            rule.if_layers.remove(at);
                                        }
                                        None => rule.if_layers.push(id),
                                    }
                                });
                            },
                        ))
                    })
                    .collect::<Vec<_>>();
                let then = layers
                    .iter()
                    .map(|(id, name)| {
                        let id = *id;
                        chip(
                            ("rule-then", (index << 16) | id.0 as usize),
                            name.clone(),
                            rule.then_layer == id,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.edit_rule(index, window, cx, |rule| rule.then_layer = id);
                            },
                        ))
                    })
                    .collect::<Vec<_>>();
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("When all of these layers are on"),
                    )
                    .child(div().flex().flex_wrap().gap_1().children(when))
                    .child(div().text_xs().text_color(muted).child("also turn on"))
                    .child(div().flex().flex_wrap().gap_1().children(then))
                    .child(
                        chip(("rule-remove", index), "Remove Rule", false, cx).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.change("Remove Layer Rule", window, cx, |p| {
                                    if index < p.conditional_layers.len() {
                                        p.conditional_layers.remove(index);
                                    }
                                    Ok(())
                                });
                            }),
                        ),
                    )
            });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_lg().child("Layer rules"))
            .child(div().text_sm().text_color(muted).child(
                "Turn a layer on automatically while a combination of other layers is active, such as an Adjust layer while Lower and Raise are both held.",
            ))
            .children(rules)
            .child(chip("rule-add", "New Rule", false, cx).on_click(cx.listener(|this, _, window, cx| {
                this.change("Add Layer Rule", window, cx, |p| {
                    let then_layer = p.layers.last().map(|l| l.id).unwrap_or(p.layers[0].id);
                    p.conditional_layers.push(ConditionalLayer {
                        if_layers: Vec::new(),
                        then_layer,
                    });
                    Ok(())
                });
            })))
    }

    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let features = self.features();
        let mut page = div().w(px(760.)).flex().flex_col().gap_3();
        let mut group = "";
        for setting in SETTINGS {
            if setting.requires.is_some_and(|f| !features.contains(&f)) {
                continue;
            }
            if setting.group != group {
                group = setting.group;
                page = page.child(div().pt_3().text_lg().child(group));
                if group == "Lighting" {
                    page = page.child(div().text_xs().text_color(muted).child(format!(
                        "This keyboard's brightness is limited to {}% to protect it.",
                        self.board.brightness_cap
                    )));
                }
            }
            page = page.child(self.render_setting(setting, cx));
        }
        let area = |title: &'static str,
                    help: &'static str,
                    state: &Entity<TextareaState>,
                    cx: &mut Context<Self>| {
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(section_title(title, cx))
                .child(div().text_xs().text_color(muted).child(help))
                .child(Textarea::new(state).h(px(140.)))
        };
        let page = page
            .child(div().pt_3().child(self.render_rules(cx)))
            .child(div().pt_3().text_lg().child("Advanced"))
            .child(div().text_sm().text_color(muted).child(
                "Text added here is written into the generated files as it is, for anything the editor does not cover. Behaviors defined here appear in the key picker's Custom tab.",
            ))
            .child(area("CUSTOM BEHAVIORS", "Devicetree nodes placed inside the keymap's behaviors section.", &self.raw_behaviors, cx))
            .child(area("CUSTOM DEVICETREE", "Devicetree placed at the end of the keymap file.", &self.raw_devicetree, cx))
            .child(area("EXTRA SETTINGS", "Lines added to the .conf file, such as CONFIG_ZMK_USB_LOGGING=y.", &self.raw_conf, cx));
        div()
            .id("settings")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .child(page)
    }
}
