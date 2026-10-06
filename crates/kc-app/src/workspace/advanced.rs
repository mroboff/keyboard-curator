//! The Advanced mode: layer rules, and devicetree text for anything the
//! editor does not cover. Firmware settings are not here: they belong to
//! the board.

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use kc_model::features::ConditionalLayer;

use super::{chip, heading, help, section_title, Workspace};

pub(super) fn subscribe(
    raw_behaviors: &Entity<TextareaState>,
    raw_devicetree: &Entity<TextareaState>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // Text is committed when focus leaves the field.
    for area in [raw_behaviors, raw_devicetree] {
        cx.subscribe_in(area, window, |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::Blur) {
                this.after_change(window, cx);
            }
        })
        .detach();
    }
}

impl Workspace {
    /// Applies what has been typed into the custom devicetree fields.
    pub(super) fn commit_advanced_inputs(&mut self, cx: &mut Context<Self>) {
        let behaviors = self.raw_behaviors.read(cx).value().to_string();
        let devicetree = self.raw_devicetree.read(cx).value().to_string();
        let raw = &self.project().raw;
        if behaviors == raw.behaviors && devicetree == raw.devicetree {
            return;
        }
        let _ = self.editor.edit("Change Custom Devicetree", |p| {
            p.raw.behaviors = behaviors;
            p.raw.devicetree = devicetree;
            Ok(())
        });
    }

    /// Adds an add-on's starter to Custom Behaviors, as one undo step.
    fn insert_starter(&mut self, snippet: String, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_advanced_inputs(cx);
        self.change("Insert Add-on Starter", window, cx, |p| {
            if !p.raw.behaviors.trim().is_empty() {
                p.raw.behaviors.push_str("\n\n");
            }
            p.raw.behaviors.push_str(&snippet);
            p.raw.behaviors.push('\n');
            Ok(())
        });
    }

    pub(super) fn sync_advanced_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.project().raw.clone();
        for (area, text) in [
            (&self.raw_behaviors, raw.behaviors),
            (&self.raw_devicetree, raw.devicetree),
        ] {
            if area.read(cx).value() != text {
                area.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
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
            .child(heading("Layer rules", cx))
            .child(help("Turn a layer on automatically while a combination of other layers is active, such as an Adjust layer while Lower and Raise are both held.", cx))
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

    pub(super) fn render_advanced(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
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
        // Starters for the board's add-ons whose behaviors the user defines.
        let behaviors = self.project().raw.behaviors.clone();
        let starters = self
            .config
            .active_addons(&self.board)
            .into_iter()
            .enumerate()
            .filter_map(|(index, addon)| {
                let snippet = addon.snippet?;
                // The label a starter defines; one already there is not
                // offered again.
                let label = snippet.split(':').next()?.trim().to_string();
                if behaviors.contains(&format!("{label}:")) {
                    return None;
                }
                Some(
                    chip(
                        ("starter", index),
                        format!("Insert {} Starter", addon.name),
                        false,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_starter(snippet.clone(), window, cx);
                    })),
                )
            })
            .collect::<Vec<_>>();
        let page = div()
            .w(px(760.))
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_rules(cx))
            .child(heading("Custom devicetree", cx).pt_8())
            .child(help("Text added here is written into the keymap file as it is, for anything the editor does not cover. Behaviors defined here appear in the key picker's Custom tab. Firmware settings are on the board's page.", cx))
            .when(!starters.is_empty(), |page| {
                page.child(help("This board's firmware has add-ons whose behaviors you define here. A starter gives you a working example to change; the behavior then appears in the key picker's Custom tab.", cx))
                .child(div().flex().flex_wrap().gap_1().children(starters))
            })
            .child(area("CUSTOM BEHAVIORS", "Devicetree nodes placed inside the keymap's behaviors section.", &self.raw_behaviors, cx))
            .child(area("CUSTOM DEVICETREE", "Devicetree placed at the end of the keymap file.", &self.raw_devicetree, cx));
        div()
            .id("advanced")
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
