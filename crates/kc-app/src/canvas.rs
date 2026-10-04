//! The keyboard canvas: draws a physical layout with its keycaps and maps
//! pointer positions back to keys.

use gpui_kit::*;
use kc_boards::geometry::{self, Key, Point as LayoutPoint};
use kc_model::keycap::{Keycap, KeycapKind};

/// Space kept clear around the keyboard, in pixels.
const PADDING: f32 = 28.;
/// Gap between neighbouring keys, in layout units.
const GAP: f32 = 4.;
/// Corner rounding of a keycap, in layout units.
const RADIUS: f32 = 12.;

/// Maps layout units into canvas bounds, preserving aspect ratio.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    scale: f32,
    origin: Point<Pixels>,
}

impl Viewport {
    pub fn fit(keys: &[Key], devices: &[Device], bounds: Bounds<Pixels>) -> Option<Self> {
        let mut rect = geometry::bounds(keys)?;
        for device in devices {
            let half = device.size / 2.;
            rect.min.x = rect.min.x.min(device.x - half);
            rect.min.y = rect.min.y.min(device.y - half);
            rect.max.x = rect.max.x.max(device.x + half);
            rect.max.y = rect.max.y.max(device.y + half);
        }
        let width = f32::from(bounds.size.width) - 2. * PADDING;
        let height = f32::from(bounds.size.height) - 2. * PADDING;
        let scale = (width / rect.width())
            .min(height / rect.height())
            .clamp(0.05, 1.2);
        let origin = point(
            bounds.origin.x
                + px(PADDING + (width - rect.width() * scale) / 2. - rect.min.x * scale),
            bounds.origin.y
                + px(PADDING + (height - rect.height() * scale) / 2. - rect.min.y * scale),
        );
        Some(Self { scale, origin })
    }

    fn to_screen(self, p: LayoutPoint) -> Point<Pixels> {
        point(
            self.origin.x + px(p.x * self.scale),
            self.origin.y + px(p.y * self.scale),
        )
    }

    fn to_layout(self, p: Point<Pixels>) -> LayoutPoint {
        LayoutPoint {
            x: f32::from(p.x - self.origin.x) / self.scale,
            y: f32::from(p.y - self.origin.y) / self.scale,
        }
    }
}

/// A trackball or touchpad, drawn alongside the keys.
#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    /// Trackballs are round; touchpads are rounded squares.
    pub round: bool,
    pub x: f32,
    pub y: f32,
    pub size: f32,
}

/// The key under a pointer position, if any.
pub fn key_at(
    keys: &[Key],
    devices: &[Device],
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
) -> Option<usize> {
    let view = Viewport::fit(keys, devices, bounds)?;
    geometry::key_at(keys, view.to_layout(position))
}

/// The keys whose centres lie inside the rectangle spanned by two pointer
/// positions.
pub fn keys_in_band(
    keys: &[Key],
    devices: &[Device],
    bounds: Bounds<Pixels>,
    band: (Point<Pixels>, Point<Pixels>),
) -> Vec<usize> {
    let Some(view) = Viewport::fit(keys, devices, bounds) else {
        return Vec::new();
    };
    let (a, b) = (view.to_layout(band.0), view.to_layout(band.1));
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    keys.iter()
        .enumerate()
        .filter(|(_, key)| {
            let c = key.center();
            (x0..=x1).contains(&c.x) && (y0..=y1).contains(&c.y)
        })
        .map(|(index, _)| index)
        .collect()
}

/// Colours the canvas draws with, taken from the active theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub key: Hsla,
    pub key_border: Hsla,
    pub text: Hsla,
    pub muted_text: Hsla,
    pub accent: Hsla,
    pub layer_key: Hsla,
}

/// Everything needed to paint one frame of the keyboard.
pub struct Frame {
    pub keys: Vec<Key>,
    pub keycaps: Vec<Keycap>,
    pub devices: Vec<Device>,
    pub selected: Vec<usize>,
    pub hovered: Option<usize>,
    /// A key being dragged, and where the pointer is.
    pub drag: Option<(usize, Point<Pixels>)>,
    /// A selection rectangle being dragged out.
    pub band: Option<(Point<Pixels>, Point<Pixels>)>,
    pub palette: Palette,
    /// A colour tag per key, when the layer view wants one.
    pub tint: Option<Hsla>,
    /// Groups of keys to join with a line, such as combos, and whether
    /// each is the one being edited.
    pub links: Vec<(Vec<usize>, bool)>,
}

/// The outline of a key, inset by the key gap, with rounded corners.
fn key_path(key: &Key, view: Viewport, stroke: Option<Pixels>) -> Option<Path<Pixels>> {
    let (gap, half) = (GAP as i32, GAP as i32 / 2);
    let corners = Key {
        x: key.x + half,
        y: key.y + half,
        w: key.w - gap,
        h: key.h - gap,
        ..*key
    }
    .corners();
    let mut builder = match stroke {
        Some(width) => PathBuilder::stroke(width),
        None => PathBuilder::fill(),
    };
    for i in 0..4 {
        let (prev, corner, next) = (corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]);
        let toward = |to: LayoutPoint| {
            let (dx, dy) = (to.x - corner.x, to.y - corner.y);
            let length = (dx * dx + dy * dy).sqrt().max(1.);
            LayoutPoint {
                x: corner.x + dx / length * RADIUS,
                y: corner.y + dy / length * RADIUS,
            }
        };
        let (start, end) = (view.to_screen(toward(prev)), view.to_screen(toward(next)));
        if i == 0 {
            builder.move_to(start);
        } else {
            builder.line_to(start);
        }
        builder.curve_to(end, view.to_screen(corner));
    }
    builder.close();
    builder.build().ok()
}

/// Paints `text` centred on `center`, shrinking it to fit `max_width`.
fn paint_text(
    text: &str,
    center: Point<Pixels>,
    mut size: Pixels,
    max_width: Pixels,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let font = window.text_style().font();
    let shape = |size: Pixels, window: &mut Window| {
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(text.to_string().into(), size, &[run], None)
    };
    let mut line = shape(size, window);
    if line.width > max_width {
        size = (size * (f32::from(max_width) / f32::from(line.width))).max(px(7.));
        line = shape(size, window);
    }
    let line_height = size * 1.25;
    let origin = point(center.x - line.width / 2., center.y - line_height / 2.);
    let _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
}

fn paint(frame: &Frame, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let Some(view) = Viewport::fit(&frame.keys, &frame.devices, bounds) else {
        return;
    };
    let palette = frame.palette;
    let unit = px(view.scale * 100.);
    for device in &frame.devices {
        let size = px(device.size * view.scale);
        let center = view.to_screen(LayoutPoint {
            x: device.x,
            y: device.y,
        });
        let radius = if device.round { size / 2. } else { size * 0.16 };
        window.paint_quad(quad(
            Bounds::centered_at(center, gpui_kit::size(size, size)),
            radius,
            palette.key.opacity(0.3),
            px(1.),
            palette.key_border,
            Default::default(),
        ));
        paint_text(
            &device.name,
            center,
            unit * 0.15,
            size * 0.9,
            palette.muted_text,
            window,
            cx,
        );
    }
    for (index, key) in frame.keys.iter().enumerate() {
        let Some(cap) = frame.keycaps.get(index) else {
            continue;
        };
        let ghost = matches!(cap.kind, KeycapKind::Transparent | KeycapKind::None);
        let fill = match cap.kind {
            KeycapKind::Layer => palette.layer_key,
            _ if ghost => palette.key.opacity(0.35),
            _ => palette.key,
        };
        let fill = match frame.tint {
            Some(tint) if !ghost => fill.blend(tint.opacity(0.16)),
            _ => fill,
        };
        let fill = if frame.hovered == Some(index) {
            fill.blend(palette.accent.opacity(0.18))
        } else {
            fill
        };
        if let Some(path) = key_path(key, view, None) {
            window.paint_path(path, fill);
        }
        let selected = frame.selected.contains(&index);
        let (width, color) = if selected {
            (px(2.5), palette.accent)
        } else {
            (px(1.), palette.key_border)
        };
        if let Some(path) = key_path(key, view, Some(width)) {
            window.paint_path(path, color);
        }

        let center = view.to_screen(key.center());
        let text = if ghost {
            palette.muted_text
        } else {
            palette.text
        };
        let max_width = unit * 0.86;
        match &cap.hold {
            Some(hold) => {
                let up = point(center.x, center.y - unit * 0.14);
                let down = point(center.x, center.y + unit * 0.24);
                paint_text(&cap.legend, up, unit * 0.26, max_width, text, window, cx);
                paint_text(
                    hold,
                    down,
                    unit * 0.17,
                    max_width,
                    palette.muted_text,
                    window,
                    cx,
                );
            }
            None => paint_text(
                &cap.legend,
                center,
                unit * 0.28,
                max_width,
                text,
                window,
                cx,
            ),
        }
    }

    for (positions, active) in &frame.links {
        let centers: Vec<Point<Pixels>> = positions
            .iter()
            .filter_map(|p| frame.keys.get(*p))
            .map(|key| view.to_screen(key.center()))
            .collect();
        if centers.len() < 2 {
            continue;
        }
        let color = if *active {
            palette.accent
        } else {
            palette.muted_text.opacity(0.6)
        };
        let mut line = PathBuilder::stroke(px(if *active { 3. } else { 2. }));
        line.move_to(centers[0]);
        for center in &centers[1..] {
            line.line_to(*center);
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, color);
        }
        for center in centers {
            let dot = Bounds::centered_at(center, gpui_kit::size(unit * 0.16, unit * 0.16));
            window.paint_quad(quad(
                dot,
                unit * 0.08,
                color,
                px(0.),
                color,
                Default::default(),
            ));
        }
    }

    if let Some((a, b)) = frame.band {
        let band = Bounds::from_corners(
            point(a.x.min(b.x), a.y.min(b.y)),
            point(a.x.max(b.x), a.y.max(b.y)),
        );
        window.paint_quad(quad(
            band,
            px(2.),
            palette.accent.opacity(0.12),
            px(1.),
            palette.accent,
            Default::default(),
        ));
    }
    if let Some((index, position)) = frame.drag {
        let ghost = Bounds::centered_at(position, gpui_kit::size(unit * 0.9, unit * 0.9));
        window.paint_quad(quad(
            ghost,
            unit * 0.1,
            palette.key.opacity(0.85),
            px(2.),
            palette.accent,
            Default::default(),
        ));
        if let Some(cap) = frame.keycaps.get(index) {
            paint_text(
                &cap.legend,
                position,
                unit * 0.26,
                unit * 0.8,
                palette.text,
                window,
                cx,
            );
        }
    }
}

/// The keyboard as an element. `on_bounds` is told where it was laid out so
/// that pointer positions can be mapped back to keys.
pub fn keyboard(frame: Frame, on_bounds: impl Fn(Bounds<Pixels>) + 'static) -> impl IntoElement {
    canvas(
        move |bounds, _, _| on_bounds(bounds),
        move |bounds, _, window, cx| paint(&frame, bounds, window, cx),
    )
    .size_full()
}
