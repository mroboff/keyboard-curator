//! Prints the keycode catalog, for eyeballing categories and legends.

fn main() {
    for key in kc_zmk::keycodes::keycodes().all() {
        println!(
            "{:?}\t{}\t{}\t{}",
            key.category,
            key.short_name(),
            key.legend,
            key.description
        );
    }
}
