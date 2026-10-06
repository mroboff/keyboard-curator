//! KEY-2 spike: prove the GPUI stack can draw and edit a keyboard canvas.
//!
//! Draws the Imprint 82-key layout (including the rotated thumb arcs), with
//! hover, click-to-select, drag-to-swap, a native menu, a text input for the
//! selected key's legend and a color picker for its color. Throwaway code:
//! the real geometry lives in `kc-boards` and the real canvas in `kc-app`.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

actions!(canvas_spike, [Quit]);

/// One key in ZMK physical-layout units: hundredths of a key unit and
/// hundredths of a degree, rotated about (`rx`, `ry`).
#[derive(Clone, Copy)]
struct KeyGeom {
    x: f32,
    y: f32,
    rot: f32,
    rx: f32,
    ry: f32,
}

impl KeyGeom {
    fn flat(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            rot: 0.,
            rx: 0.,
            ry: 0.,
        }
    }

    /// Corners in layout units, clockwise from top-left, after rotation.
    fn corners(&self) -> [(f32, f32); 4] {
        let (sin, cos) = (self.rot / 100.).to_radians().sin_cos();
        [(0., 0.), (100., 0.), (100., 100.), (0., 100.)].map(|(dx, dy)| {
            let (px, py) = (self.x + dx - self.rx, self.y + dy - self.ry);
            (self.rx + px * cos - py * sin, self.ry + px * sin + py * cos)
        })
    }
}

/// The Imprint `function_row_full_bottom_row` layout, in binding order.
fn imprint_82() -> Vec<KeyGeom> {
    const STAGGER: [f32; 6] = [125., 125., 50., 0., 25., 25.];
    let mut keys = Vec::with_capacity(82);
    for row in 0..5 {
        let y = row as f32 * 100.;
        for (col, stagger) in STAGGER.iter().enumerate() {
            keys.push(KeyGeom::flat(col as f32 * 100., y + stagger));
        }
        for col in 0..6 {
            keys.push(KeyGeom::flat(
                1150. + col as f32 * 100.,
                y + STAGGER[5 - col],
            ));
        }
    }
    for (col, stagger) in STAGGER.iter().enumerate().take(5) {
        keys.push(KeyGeom::flat(col as f32 * 100., 500. + stagger));
    }
    for col in 1..6 {
        keys.push(KeyGeom::flat(
            1150. + col as f32 * 100.,
            500. + STAGGER[5 - col],
        ));
    }
    for arc in 0..2 {
        let y = 575. + arc as f32 * 100.;
        let ry = 1340. + arc as f32 * 100.;
        for rot in [0., 800., 1600.] {
            keys.push(KeyGeom {
                x: 550.,
                y,
                rot,
                rx: if rot == 0. { 0. } else { 600. },
                ry: if rot == 0. { 0. } else { ry },
            });
        }
        for rot in [-1600., -800., 0.] {
            keys.push(KeyGeom {
                x: 1100.,
                y,
                rot,
                rx: if rot == 0. { 0. } else { 1150. },
                ry: if rot == 0. { 0. } else { ry },
            });
        }
    }
    keys
}

/// Maps layout units into the canvas bounds, preserving aspect ratio.
#[derive(Clone, Copy)]
struct Viewport {
    scale: f32,
    origin: Point<Pixels>,
}

impl Viewport {
    fn fit(keys: &[KeyGeom], bounds: Bounds<Pixels>) -> Self {
        let pts = keys.iter().flat_map(|k| k.corners());
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in pts {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
        let pad = 24.;
        let (w, h) = (
            f32::from(bounds.size.width) - 2. * pad,
            f32::from(bounds.size.height) - 2. * pad,
        );
        let scale = (w / (x1 - x0)).min(h / (y1 - y0)).max(0.01);
        let origin = point(
            bounds.origin.x + px(pad + (w - (x1 - x0) * scale) / 2. - x0 * scale),
            bounds.origin.y + px(pad + (h - (y1 - y0) * scale) / 2. - y0 * scale),
        );
        Self { scale, origin }
    }

    fn to_screen(self, (x, y): (f32, f32)) -> Point<Pixels> {
        point(
            self.origin.x + px(x * self.scale),
            self.origin.y + px(y * self.scale),
        )
    }

    fn to_layout(self, p: Point<Pixels>) -> (f32, f32) {
        (
            f32::from(p.x - self.origin.x) / self.scale,
            f32::from(p.y - self.origin.y) / self.scale,
        )
    }
}

fn contains(corners: &[(f32, f32); 4], (x, y): (f32, f32)) -> bool {
    // Convex polygon with consistent winding: the point is inside when it is
    // on the same side of every edge.
    (0..4).all(|i| {
        let (ax, ay) = corners[i];
        let (bx, by) = corners[(i + 1) % 4];
        (bx - ax) * (y - ay) - (by - ay) * (x - ax) >= 0.
    })
}

#[derive(Clone)]
struct KeyState {
    legend: SharedString,
    color: Option<Hsla>,
}

struct Spike {
    geoms: Rc<Vec<KeyGeom>>,
    keys: Vec<KeyState>,
    selected: Option<usize>,
    hovered: Option<usize>,
    drag: Option<(usize, Point<Pixels>)>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    legend_input: Entity<InputState>,
    color_state: Entity<ColorPickerState>,
}

impl Spike {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let geoms = imprint_82();
        let keys = (0..geoms.len())
            .map(|i| KeyState {
                legend: format!("{i}").into(),
                color: None,
            })
            .collect();
        let legend_input = cx.new(|cx| InputState::new(window, cx).placeholder("Legend"));
        let color_state = cx.new(|cx| ColorPickerState::new(window, cx));

        cx.subscribe(&legend_input, |this, input, event: &InputEvent, cx| {
            if let (InputEvent::Change, Some(ix)) = (event, this.selected) {
                this.keys[ix].legend = input.read(cx).value();
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&color_state, |this, _, event: &ColorPickerEvent, cx| {
            let ColorPickerEvent::Change(color) = event;
            if let Some(ix) = this.selected {
                this.keys[ix].color = *color;
                cx.notify();
            }
        })
        .detach();

        Self {
            geoms: Rc::new(geoms),
            keys,
            selected: None,
            hovered: None,
            drag: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
            legend_input,
            color_state,
        }
    }

    fn key_at(&self, position: Point<Pixels>) -> Option<usize> {
        let p = Viewport::fit(&self.geoms, self.canvas_bounds.get()).to_layout(position);
        // Later keys paint on top, so search from the end.
        self.geoms.iter().rposition(|k| contains(&k.corners(), p))
    }
}

impl Render for Spike {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border, key_bg, accent) = (
            theme.background,
            theme.foreground,
            theme.border,
            theme.secondary,
            theme.primary,
        );
        let geoms = self.geoms.clone();
        let keys = self.keys.clone();
        let (selected, hovered, drag) = (self.selected, self.hovered, self.drag);
        let bounds_cell = self.canvas_bounds.clone();

        let board = canvas(
            move |bounds, _, _| bounds_cell.set(bounds),
            move |bounds, _, window, cx| {
                let view = Viewport::fit(&geoms, bounds);
                let font = window.text_style().font();
                let font_size = px((view.scale * 26.).clamp(8., 18.));
                for (ix, geom) in geoms.iter().enumerate() {
                    let corners = geom.corners().map(|c| view.to_screen(c));
                    let fill = keys[ix].color.unwrap_or(key_bg);
                    let fill = if hovered == Some(ix) {
                        fill.opacity(0.75)
                    } else {
                        fill
                    };
                    let mut body = PathBuilder::fill();
                    body.add_polygon(&corners, true);
                    if let Ok(path) = body.build() {
                        window.paint_path(path, fill);
                    }
                    let is_selected = selected == Some(ix);
                    let mut outline = PathBuilder::stroke(px(if is_selected { 2.5 } else { 1. }));
                    outline.add_polygon(&corners, true);
                    if let Ok(path) = outline.build() {
                        window.paint_path(path, if is_selected { accent } else { border });
                    }

                    let legend = keys[ix].legend.clone();
                    if legend.is_empty() {
                        continue;
                    }
                    // Readable legend color against the key's own fill.
                    let color = if keys[ix].color.is_some_and(|c| c.l < 0.5) {
                        gpui_kit::white()
                    } else if keys[ix].color.is_some() {
                        gpui_kit::black()
                    } else {
                        fg
                    };
                    let run = TextRun {
                        len: legend.len(),
                        font: font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let line = window
                        .text_system()
                        .shape_line(legend, font_size, &[run], None);
                    let center = point(
                        (corners[0].x + corners[2].x) / 2.,
                        (corners[0].y + corners[2].y) / 2.,
                    );
                    let line_height = font_size * 1.2;
                    let origin = point(center.x - line.width / 2., center.y - line_height / 2.);
                    let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
                }

                if let Some((ix, position)) = drag {
                    let size = px(view.scale * 100.);
                    let ghost = Bounds::centered_at(position, gpui_kit::size(size, size));
                    window.paint_quad(quad(
                        ghost,
                        px(4.),
                        keys[ix].color.unwrap_or(key_bg).opacity(0.6),
                        px(1.5),
                        accent,
                        Default::default(),
                    ));
                }
            },
        )
        .size_full();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(fg)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .border_b_1()
                    .border_color(border)
                    .child(match selected {
                        Some(ix) => format!("Key {ix}"),
                        None => "Click a key; drag one onto another to swap".into(),
                    })
                    .child(div().w_48().child(Input::new(&self.legend_input)))
                    .child(ColorPicker::new(&self.color_state)),
            )
            .child(
                div()
                    .flex_1()
                    .child(board)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.selected = this.key_at(event.position);
                            if let Some(ix) = this.selected {
                                this.drag = Some((ix, event.position));
                                let legend = this.keys[ix].legend.clone();
                                this.legend_input.update(cx, |input, cx| {
                                    input.set_value(legend, window, cx);
                                });
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        let hovered = this.key_at(event.position);
                        if let Some((_, position)) = this.drag.as_mut() {
                            *position = event.position;
                            cx.notify();
                        }
                        if hovered != this.hovered {
                            this.hovered = hovered;
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            if let Some((from, _)) = this.drag.take() {
                                if let Some(to) = this.key_at(event.position) {
                                    if to != from {
                                        this.keys.swap(from, to);
                                        this.selected = Some(to);
                                    }
                                }
                            }
                            cx.notify();
                        }),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus([Menu::new("Keyboard Curator").items([
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Quit Keyboard Curator", Quit),
        ])]);

        let bounds = Bounds::centered(None, size(px(1100.), px(620.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Spike::new(window, cx))
        })
        .expect("failed to open window");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
