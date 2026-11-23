//! The lighting reports of the devices in [`super::devices`], checked on one
//! unit of each. No I/O.
//!
//! The control interface takes the 90-byte control reports of the ARGB
//! controller. A frame is one report per matrix row and then the report that
//! shows it. Neither one is stored: the device keeps its own effect in
//! memory, and [`restore`] puts it back.

pub use super::super::protocol::VENDOR;
use super::super::protocol::control;
use super::devices::Macros;

/// The answer to a report, report ID first, the way `get_feature_report`
/// fills it.
pub const ANSWER_LEN: usize = 91;

/// Where the answer carries its status, and the status of a report taken.
const STATUS: usize = 1;
const OK: u8 = 0x02;
/// The effect store that the device keeps over power loss, and the one in
/// use now.
const STORED: u8 = 0x01;
const LIVE: u8 = 0x00;
const CUSTOM: u8 = 0x08;
/// Argument bytes of an effect report and of its answer.
const EFFECT_ARGS: usize = 12;
const SET_EFFECT: u8 = 0x02;
const GET_EFFECT: u8 = 0x82;

/// The colors of one matrix row, column 0 first: 25 at most.
pub fn row(row: usize, colors: &[[u8; 3]]) -> Vec<u8> {
    #[allow(clippy::cast_possible_truncation)]
    let last = colors.len().saturating_sub(1) as u8;
    #[allow(clippy::cast_possible_truncation)]
    let mut args = vec![0x00, 0x00, row as u8, 0x00, last];
    args.extend(colors.iter().flatten());
    control(0x0F, 0x03, &args)
}

/// Show the rows sent since the last one.
pub fn show() -> Vec<u8> {
    effect(SET_EFFECT, &[LIVE, 0x00, CUSTOM])
}

/// Ask for the stored effect of `led`. [`restore`] takes the answer.
pub fn read_stored(led: u8) -> Vec<u8> {
    effect(GET_EFFECT, &[STORED, led])
}

/// Run the effect of a [`read_stored`] answer now, with its parameters.
/// `None` where the answer is not a success.
pub fn restore(answer: &[u8]) -> Option<Vec<u8>> {
    if !taken(answer) {
        return None;
    }
    let mut args = answer.get(9..9 + EFFECT_ARGS)?.to_vec();
    args[0] = LIVE;
    Some(effect(SET_EFFECT, &args))
}

/// Driver mode (`true`) makes the macro keys report to the app, and
/// normal mode hands them back. The device forgets it when it loses power.
pub fn driver_mode(on: bool) -> Vec<u8> {
    control(0x00, 0x04, &[if on { 0x03 } else { 0x00 }, 0x00])
}

/// The macro key that `report` presses, by its place in `macros.codes`.
/// `None` for a key that goes up and for any other report.
pub fn pressed(macros: &Macros, report: &[u8]) -> Option<usize> {
    match report {
        [id, code, ..] if *id == macros.report && *code != 0 => {
            macros.codes.iter().position(|c| c == code)
        }
        _ => None,
    }
}

/// Whether the device took the report that `answer` answers.
pub fn taken(answer: &[u8]) -> bool {
    answer.get(STATUS) == Some(&OK)
}

fn effect(id: u8, args: &[u8]) -> Vec<u8> {
    let mut padded = [0u8; EFFECT_ARGS];
    padded[..args.len()].copy_from_slice(args);
    control(0x0F, id, &padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_report_matches_the_one_the_unit_showed() {
        let report = row(2, &[[0xFF, 0, 0]; 22]);
        assert_eq!(report.len(), 91);
        assert_eq!(
            &report[1..14],
            &[
                0x00, 0x1F, 0x00, 0x00, 0x00, 0x47, 0x0F, 0x03, 0x00, 0x00, 2, 0x00, 0x15
            ]
        );
        assert_eq!(&report[14..17], &[0xFF, 0, 0]);
    }

    #[test]
    fn a_macro_report_names_its_key_and_a_release_names_none() {
        let Some(macros) = &super::super::devices::KEYBOARD.macros else {
            panic!("the keyboard has macro keys");
        };
        assert_eq!(pressed(macros, &[0x04, 0x54, 0x00]), Some(0));
        assert_eq!(pressed(macros, &[0x04, 0x55, 0x00]), Some(1));
        assert_eq!(pressed(macros, &[0x04, 0x53, 0x00]), Some(2));
        assert_eq!(pressed(macros, &[0x04, 0x00, 0x00]), None);
        assert_eq!(pressed(macros, &[0x01, 0x54, 0x00]), None);
        assert_eq!(&driver_mode(true)[6..10], &[0x02, 0x00, 0x04, 0x03]);
    }

    #[test]
    fn the_effect_reports_match_the_ones_the_unit_answered() {
        assert_eq!(&show()[6..12], &[0x0C, 0x0F, 0x02, 0x00, 0x00, 0x08]);
        assert_eq!(&read_stored(0x05)[6..11], &[0x0C, 0x0F, 0x82, 0x01, 0x05]);
        // The stored effect of the unit: 03, a color cycle.
        let mut answer = vec![0u8; ANSWER_LEN];
        answer[1] = OK;
        answer[9..12].copy_from_slice(&[0x01, 0x05, 0x03]);
        let report = restore(&answer).unwrap_or_default();
        assert_eq!(&report[6..12], &[0x0C, 0x0F, 0x02, 0x00, 0x05, 0x03]);
        answer[1] = 0x05;
        assert_eq!(restore(&answer), None);
        // The mousepad answers a stored effect with its parameters.
        answer[1] = OK;
        answer[9..14].copy_from_slice(&[0x00, 0x05, 0x04, 0x01, 0x28]);
        let report = restore(&answer).unwrap_or_default();
        assert_eq!(&report[9..14], &[0x00, 0x05, 0x04, 0x01, 0x28]);
    }
}
