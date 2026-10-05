//! Pointing knowledge that is the same on every ZMK firmware.

/// Input listeners every ZMK firmware with pointing support has, for the
/// mouse keys: `(devicetree label, name, whether it produces scrolling)`.
pub const MOUSE_KEY_LISTENERS: [(&str, &str, bool); 2] = [
    ("mmv_input_listener", "Mouse keys: pointer movement", false),
    ("msc_input_listener", "Mouse keys: scrolling", true),
];

/// The mouse-key listener with this label, if it is one.
pub fn mouse_key_listener(label: &str) -> Option<(&'static str, &'static str, bool)> {
    MOUSE_KEY_LISTENERS
        .iter()
        .copied()
        .find(|(listener, _, _)| *listener == label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_key_listeners_are_looked_up_by_label() {
        assert_eq!(
            mouse_key_listener("msc_input_listener").map(|l| l.2),
            Some(true)
        );
        assert!(mouse_key_listener("trackball_listener").is_none());
    }
}
