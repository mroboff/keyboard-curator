//! The ZMK headers a keymap includes for its constants, as vendored text.
//!
//! Reading a keymap someone else wrote means knowing what these define: a
//! file may test a name with `#ifdef` to decide what to include.

/// Include paths and the vendored text of each.
const HEADERS: [(&str, &str); 7] = [
    (
        "dt-bindings/zmk/keys.h",
        include_str!("../vendor/zmk-v0.3.0/keys.h"),
    ),
    (
        "dt-bindings/zmk/bt.h",
        include_str!("../vendor/zmk-v0.3.0/bt.h"),
    ),
    (
        "dt-bindings/zmk/outputs.h",
        include_str!("../vendor/zmk-v0.3.0/outputs.h"),
    ),
    (
        "dt-bindings/zmk/rgb.h",
        include_str!("../vendor/zmk-v0.3.0/rgb.h"),
    ),
    (
        "dt-bindings/zmk/backlight.h",
        include_str!("../vendor/zmk-v0.3.0/backlight.h"),
    ),
    (
        "dt-bindings/zmk/ext_power.h",
        include_str!("../vendor/zmk-v0.3.0/ext_power.h"),
    ),
    (
        "dt-bindings/zmk/pointing.h",
        include_str!("../vendor/zmk-v0.3.0/pointing.h"),
    ),
];

/// The text of a header, by the path an `#include` names it with.
pub fn header(include: &str) -> Option<&'static str> {
    HEADERS
        .iter()
        .find(|(path, _)| *path == include)
        .map(|(_, text)| *text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_found_by_include_path() {
        assert!(header("dt-bindings/zmk/bt.h").is_some_and(|h| h.contains("#define BT_DISC_CMD")));
        assert!(header("behaviors.dtsi").is_none());
    }
}
