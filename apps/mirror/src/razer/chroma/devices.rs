//! The Razer devices that take app-drawn effects, and where each LED sits.
//! Each matrix position was lit on one unit and checked by eye.

/// Which settings record a device reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Keyboard,
    Mousepad,
}

/// One device: how to reach it over USB, and its LEDs.
#[derive(Debug)]
pub struct Spec {
    pub device: Device,
    pub name: &'static str,
    /// The line under the name in the card.
    pub kind: &'static str,
    /// What the window calls the lit parts: "keys" or "LEDs".
    pub parts: &'static str,
    pub product: u16,
    /// The interface that takes the control reports.
    pub interface: i32,
    /// The LED that the read of the stored effect names.
    pub led: u8,
    pub rows: usize,
    /// Positions one row report carries.
    pub cols: usize,
    pub keys: &'static [Key],
    /// The keys that report to the app in driver mode, where the device has
    /// any.
    pub macros: Option<Macros>,
    /// The drawing, in key widths.
    pub width: f32,
    pub height: f32,
}

/// Keys that send nothing to Windows. In driver mode, the device reports
/// each press on a collection of its own.
#[derive(Debug)]
pub struct Macros {
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
    /// The report ID of a press, and the code of each key, left first. The
    /// code is 0 when the key goes up.
    pub report: u8,
    pub codes: &'static [u8],
    pub labels: &'static [&'static str],
}

/// One LED, or one key with a light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Key {
    pub label: &'static str,
    /// The position in the lighting matrix.
    pub row: u8,
    pub col: u8,
    /// The drawing, in key widths: the left edge, the top edge, the width and
    /// the height.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Where the Wave effect finds the key, from 0 to 1.
    pub along: f32,
}

pub const ALL: [&Spec; 2] = [&KEYBOARD, &MOUSEPAD];

/// The Huntsman V2, full size, ANSI. Matrix column 21 has no key.
pub const KEYBOARD: Spec = Spec {
    device: Device::Keyboard,
    name: "Huntsman V2",
    kind: "Keyboard · key effects",
    parts: "keys",
    product: 0x026C,
    interface: 3,
    led: 0x05,
    rows: 6,
    cols: 22,
    keys: KEYS,
    macros: Some(Macros {
        interface: 1,
        usage_page: 0x0001,
        usage: 0x0000,
        report: 0x04,
        codes: &[0x54, 0x55, 0x53],
        labels: &["Left macro key", "Middle macro key", "Right macro key"],
    }),
    width: KEYBOARD_WIDTH,
    height: 6.5,
};

/// The Strider Chroma: one strip of 19 LEDs up the left edge, LED 0 at the
/// bottom. The drawing gives the pad its 900 × 400 mm shape.
pub const MOUSEPAD: Spec = Spec {
    device: Device::Mousepad,
    name: "Strider Chroma",
    kind: "Mousepad · LED effects",
    parts: "LEDs",
    product: 0x0C05,
    interface: 0,
    led: 0x00,
    rows: 1,
    cols: PAD_LEDS,
    keys: &PAD,
    macros: None,
    width: 21.4,
    height: PAD_LEDS as f32 * 0.5,
};

const KEYBOARD_WIDTH: f32 = 22.5;
const PAD_LEDS: usize = 19;

const PAD: [Key; PAD_LEDS] = {
    let mut keys = [led(0); PAD_LEDS];
    let mut index = 1;
    while index < PAD_LEDS {
        keys[index] = led(index);
        index += 1;
    }
    keys
};

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
const fn led(index: usize) -> Key {
    Key {
        label: "",
        row: 0,
        col: index as u8,
        x: 0.0,
        y: (PAD_LEDS - 1 - index) as f32 * 0.5,
        w: 0.5,
        h: 0.5,
        along: index as f32 / (PAD_LEDS - 1) as f32,
    }
}

/// The function row sits half a key above the others.
const fn key(label: &'static str, row: u8, col: u8, x: f32, w: f32) -> Key {
    let y = if row == 0 { 0.0 } else { row as f32 + 0.5 };
    Key {
        label,
        row,
        col,
        x,
        y,
        w,
        h: 1.0,
        along: (x + w / 2.0) / KEYBOARD_WIDTH,
    }
}

const fn tall(label: &'static str, row: u8, col: u8, x: f32) -> Key {
    let mut key = key(label, row, col, x, 1.0);
    key.h = 2.0;
    key
}

#[rustfmt::skip]
const KEYS: &[Key] = &[
    key("Esc", 0, 0, 0.0, 1.0),
    key("F1", 0, 2, 2.0, 1.0), key("F2", 0, 3, 3.0, 1.0), key("F3", 0, 4, 4.0, 1.0),
    key("F4", 0, 5, 5.0, 1.0), key("F5", 0, 6, 6.5, 1.0), key("F6", 0, 7, 7.5, 1.0),
    key("F7", 0, 8, 8.5, 1.0), key("F8", 0, 9, 9.5, 1.0), key("F9", 0, 10, 11.0, 1.0),
    key("F10", 0, 11, 12.0, 1.0), key("F11", 0, 12, 13.0, 1.0), key("F12", 0, 13, 14.0, 1.0),
    key("PrtSc", 0, 14, 15.25, 1.0), key("ScrLk", 0, 15, 16.25, 1.0), key("Pause", 0, 16, 17.25, 1.0),
    key("◀◀", 0, 17, 18.5, 1.0), key("▶", 0, 18, 19.5, 1.0), key("▶▶", 0, 19, 20.5, 1.0),
    key("Vol", 0, 20, 21.5, 1.0),

    key("`", 1, 0, 0.0, 1.0), key("1", 1, 1, 1.0, 1.0), key("2", 1, 2, 2.0, 1.0),
    key("3", 1, 3, 3.0, 1.0), key("4", 1, 4, 4.0, 1.0), key("5", 1, 5, 5.0, 1.0),
    key("6", 1, 6, 6.0, 1.0), key("7", 1, 7, 7.0, 1.0), key("8", 1, 8, 8.0, 1.0),
    key("9", 1, 9, 9.0, 1.0), key("0", 1, 10, 10.0, 1.0), key("-", 1, 11, 11.0, 1.0),
    key("=", 1, 12, 12.0, 1.0), key("Bksp", 1, 13, 13.0, 2.0),
    key("Ins", 1, 14, 15.25, 1.0), key("Home", 1, 15, 16.25, 1.0), key("PgUp", 1, 16, 17.25, 1.0),
    key("Num", 1, 17, 18.5, 1.0), key("/", 1, 18, 19.5, 1.0), key("*", 1, 19, 20.5, 1.0),
    key("-", 1, 20, 21.5, 1.0),

    key("Tab", 2, 0, 0.0, 1.5), key("Q", 2, 1, 1.5, 1.0), key("W", 2, 2, 2.5, 1.0),
    key("E", 2, 3, 3.5, 1.0), key("R", 2, 4, 4.5, 1.0), key("T", 2, 5, 5.5, 1.0),
    key("Y", 2, 6, 6.5, 1.0), key("U", 2, 7, 7.5, 1.0), key("I", 2, 8, 8.5, 1.0),
    key("O", 2, 9, 9.5, 1.0), key("P", 2, 10, 10.5, 1.0), key("[", 2, 11, 11.5, 1.0),
    key("]", 2, 12, 12.5, 1.0), key("\\", 2, 13, 13.5, 1.5),
    key("Del", 2, 14, 15.25, 1.0), key("End", 2, 15, 16.25, 1.0), key("PgDn", 2, 16, 17.25, 1.0),
    key("7", 2, 17, 18.5, 1.0), key("8", 2, 18, 19.5, 1.0), key("9", 2, 19, 20.5, 1.0),
    tall("+", 2, 20, 21.5),

    key("Caps", 3, 0, 0.0, 1.75), key("A", 3, 1, 1.75, 1.0), key("S", 3, 2, 2.75, 1.0),
    key("D", 3, 3, 3.75, 1.0), key("F", 3, 4, 4.75, 1.0), key("G", 3, 5, 5.75, 1.0),
    key("H", 3, 6, 6.75, 1.0), key("J", 3, 7, 7.75, 1.0), key("K", 3, 8, 8.75, 1.0),
    key("L", 3, 9, 9.75, 1.0), key(";", 3, 10, 10.75, 1.0), key("'", 3, 11, 11.75, 1.0),
    key("Enter", 3, 13, 12.75, 2.25),
    key("4", 3, 17, 18.5, 1.0), key("5", 3, 18, 19.5, 1.0), key("6", 3, 19, 20.5, 1.0),

    key("Shift", 4, 0, 0.0, 2.25), key("Z", 4, 2, 2.25, 1.0), key("X", 4, 3, 3.25, 1.0),
    key("C", 4, 4, 4.25, 1.0), key("V", 4, 5, 5.25, 1.0), key("B", 4, 6, 6.25, 1.0),
    key("N", 4, 7, 7.25, 1.0), key("M", 4, 8, 8.25, 1.0), key(",", 4, 9, 9.25, 1.0),
    key(".", 4, 10, 10.25, 1.0), key("/", 4, 11, 11.25, 1.0), key("Shift", 4, 13, 12.25, 2.75),
    key("↑", 4, 15, 16.25, 1.0),
    key("1", 4, 17, 18.5, 1.0), key("2", 4, 18, 19.5, 1.0), key("3", 4, 19, 20.5, 1.0),
    tall("Ent", 4, 20, 21.5),

    key("Ctrl", 5, 0, 0.0, 1.25), key("Win", 5, 1, 1.25, 1.25), key("Alt", 5, 2, 2.5, 1.25),
    key("", 5, 6, 3.75, 6.25), key("Alt", 5, 10, 10.0, 1.25), key("Fn", 5, 11, 11.25, 1.25),
    key("Menu", 5, 12, 12.5, 1.25), key("Ctrl", 5, 13, 13.75, 1.25),
    key("←", 5, 14, 15.25, 1.0), key("↓", 5, 15, 16.25, 1.0), key("→", 5, 16, 17.25, 1.0),
    key("0", 5, 18, 18.5, 2.0), key(".", 5, 19, 20.5, 1.0),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_its_own_matrix_position_inside_the_drawing() {
        for spec in ALL {
            let mut seen = std::collections::HashSet::new();
            for key in spec.keys {
                assert!(seen.insert((key.row, key.col)), "{} twice", key.label);
                assert!(usize::from(key.row) < spec.rows && usize::from(key.col) < spec.cols);
                assert!(key.x + key.w <= spec.width && key.y + key.h <= spec.height);
                assert!((0.0..=1.0).contains(&key.along));
            }
        }
        assert_eq!(KEYBOARD.keys.len(), 108);
        assert_eq!(MOUSEPAD.keys.len(), 19);
    }

    #[test]
    fn the_pad_strip_runs_from_the_bottom_up() {
        let (first, last) = (MOUSEPAD.keys[0], MOUSEPAD.keys[18]);
        assert!(first.y > last.y);
        assert_eq!((first.along, last.along), (0.0, 1.0));
    }
}
