//! Key geometry in ZMK physical-layout units: hundredths of a key unit for
//! lengths and hundredths of a degree for rotation.

use serde::{Deserialize, Serialize};

/// A point in layout units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// An axis-aligned rectangle in layout units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Point,
    pub max: Point,
}

impl Rect {
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }
}

/// One key of a physical layout, as ZMK's `key_physical_attrs` describes it.
///
/// The key is a `w` by `h` rectangle with its top-left corner at (`x`, `y`),
/// rotated clockwise by `rot` about the absolute point (`rx`, `ry`).
/// Serialized as `[w, h, x, y, rot, rx, ry]`, the same order ZMK uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "[i32; 7]", into = "[i32; 7]")]
pub struct Key {
    pub w: i32,
    pub h: i32,
    pub x: i32,
    pub y: i32,
    pub rot: i32,
    pub rx: i32,
    pub ry: i32,
}

impl From<[i32; 7]> for Key {
    fn from([w, h, x, y, rot, rx, ry]: [i32; 7]) -> Self {
        Self {
            w,
            h,
            x,
            y,
            rot,
            rx,
            ry,
        }
    }
}

impl From<Key> for [i32; 7] {
    fn from(k: Key) -> Self {
        [k.w, k.h, k.x, k.y, k.rot, k.rx, k.ry]
    }
}

impl Key {
    fn rotate(&self, x: f32, y: f32) -> Point {
        if self.rot == 0 {
            return Point { x, y };
        }
        let (sin, cos) = (self.rot as f32 / 100.).to_radians().sin_cos();
        let (rx, ry) = (self.rx as f32, self.ry as f32);
        let (dx, dy) = (x - rx, y - ry);
        Point {
            x: rx + dx * cos - dy * sin,
            y: ry + dx * sin + dy * cos,
        }
    }

    /// The key's outline after rotation, clockwise from its top-left corner.
    pub fn corners(&self) -> [Point; 4] {
        let (x, y, w, h) = (self.x as f32, self.y as f32, self.w as f32, self.h as f32);
        [
            self.rotate(x, y),
            self.rotate(x + w, y),
            self.rotate(x + w, y + h),
            self.rotate(x, y + h),
        ]
    }

    /// The key's centre after rotation.
    pub fn center(&self) -> Point {
        self.rotate(
            self.x as f32 + self.w as f32 / 2.,
            self.y as f32 + self.h as f32 / 2.,
        )
    }

    /// Whether `p` lies inside the rotated outline.
    pub fn contains(&self, p: Point) -> bool {
        let corners = self.corners();
        (0..4).all(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x) >= 0.
        })
    }
}

/// The smallest rectangle containing every key's rotated outline, or `None`
/// for an empty layout.
pub fn bounds(keys: &[Key]) -> Option<Rect> {
    let mut points = keys.iter().flat_map(Key::corners);
    let first = points.next()?;
    Some(points.fold(
        Rect {
            min: first,
            max: first,
        },
        |r, p| Rect {
            min: Point {
                x: r.min.x.min(p.x),
                y: r.min.y.min(p.y),
            },
            max: Point {
                x: r.max.x.max(p.x),
                y: r.max.y.max(p.y),
            },
        },
    ))
}

/// The topmost key under `p`. Later keys draw on top, so they win.
pub fn key_at(keys: &[Key], p: Point) -> Option<usize> {
    keys.iter().rposition(|k| k.contains(p))
}

/// A direction to move the selection in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The key that is the natural neighbour of `from` in `direction`, judged by
/// key centres: the nearest key that way, preferring ones in line.
pub fn neighbor(keys: &[Key], from: usize, direction: Direction) -> Option<usize> {
    let origin = keys.get(from)?.center();
    let (dx, dy) = match direction {
        Direction::Left => (-1., 0.),
        Direction::Right => (1., 0.),
        Direction::Up => (0., -1.),
        Direction::Down => (0., 1.),
    };
    keys.iter()
        .enumerate()
        .filter(|(index, _)| *index != from)
        .filter_map(|(index, key)| {
            let center = key.center();
            let (vx, vy) = (center.x - origin.x, center.y - origin.y);
            let along = vx * dx + vy * dy;
            let across = (vx * dy - vy * dx).abs();
            // Must be clearly that way, and more that way than sideways.
            (along > 25. && across < along * 1.5).then_some((index, along + 2. * across))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbours_follow_the_physical_grid() {
        // A 3x2 grid with the middle column dropped by a quarter key.
        let keys: Vec<Key> = [
            (0, 0),
            (100, 25),
            (200, 0),
            (0, 100),
            (100, 125),
            (200, 100),
        ]
        .iter()
        .map(|&(x, y)| Key::from([100, 100, x, y, 0, 0, 0]))
        .collect();
        assert_eq!(neighbor(&keys, 0, Direction::Right), Some(1));
        assert_eq!(neighbor(&keys, 1, Direction::Right), Some(2));
        assert_eq!(neighbor(&keys, 2, Direction::Right), None);
        assert_eq!(neighbor(&keys, 1, Direction::Down), Some(4));
        assert_eq!(neighbor(&keys, 4, Direction::Up), Some(1));
        assert_eq!(neighbor(&keys, 5, Direction::Left), Some(4));
        assert_eq!(neighbor(&keys, 0, Direction::Up), None);
        assert_eq!(neighbor(&keys, 9, Direction::Up), None);
    }

    fn close(a: Point, x: f32, y: f32) -> bool {
        (a.x - x).abs() < 0.01 && (a.y - y).abs() < 0.01
    }

    #[test]
    fn unrotated_key_keeps_its_rectangle() {
        let key = Key::from([100, 100, 200, 50, 0, 0, 0]);
        let c = key.corners();
        assert!(close(c[0], 200., 50.) && close(c[2], 300., 150.));
        assert!(close(key.center(), 250., 100.));
    }

    #[test]
    fn rotation_is_clockwise_about_the_pivot() {
        // A quarter turn about the key's own top-left corner swings the
        // top-right corner to directly below the pivot.
        let key = Key::from([100, 100, 0, 0, 9000, 0, 0]);
        let c = key.corners();
        assert!(close(c[0], 0., 0.));
        assert!(close(c[1], 0., 100.));
        assert!(close(c[3], -100., 0.));
    }

    #[test]
    fn hit_testing_follows_rotation() {
        let key = Key::from([100, 100, 0, 0, 4500, 0, 0]);
        assert!(key.contains(Point { x: 0., y: 70. }));
        assert!(!key.contains(Point { x: 80., y: 20. }));
    }

    #[test]
    fn bounds_cover_rotated_corners() {
        let keys = [
            Key::from([100, 100, 0, 0, 0, 0, 0]),
            Key::from([100, 100, 0, 0, 9000, 0, 0]),
        ];
        let r = bounds(&keys).unwrap();
        assert!(close(r.min, -100., 0.) && close(r.max, 100., 100.));
        assert!(bounds(&[]).is_none());
    }

    #[test]
    fn later_keys_win_hit_tests() {
        let keys = [
            Key::from([100, 100, 0, 0, 0, 0, 0]),
            Key::from([100, 100, 50, 0, 0, 0, 0]),
        ];
        assert_eq!(key_at(&keys, Point { x: 75., y: 50. }), Some(1));
        assert_eq!(key_at(&keys, Point { x: 25., y: 50. }), Some(0));
        assert_eq!(key_at(&keys, Point { x: 500., y: 50. }), None);
    }
}
