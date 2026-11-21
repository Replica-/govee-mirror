//! The reports of the Razer Chroma Addressable RGB Controller, as captured
//! from Synapse and checked on one unit. No I/O.
//!
//! Two interfaces of one USB device take them:
//!
//! - Interface 0 takes 90-byte control reports: a header, a command class and
//!   id, up to 80 argument bytes, and an XOR checksum.
//! - Interface 1 takes one 320-byte frame report per port. A frame sends all
//!   six ports, and the last one carries the flag that shows the frame.

/// USB vendor and product of the controller.
pub const VENDOR: u16 = 0x1532;
pub const PRODUCT: u16 = 0x0F1F;
pub const CONTROL_INTERFACE: i32 = 0;
pub const FRAME_INTERFACE: i32 = 1;

pub const PORTS: usize = 6;
/// LEDs one frame report carries, whatever strip is on the port.
pub const LEDS: usize = 80;

const CONTROL_LEN: usize = 90;
const FRAME_LEN: usize = 320;
/// Any transaction id is answered. This is the one the probe used.
const TRANSACTION: u8 = 0x1F;

/// A control report, with the report id 0 that the HID call takes first.
fn control(class: u8, id: u8, args: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; 1 + CONTROL_LEN];
    let body = &mut r[1..];
    body[1] = TRANSACTION;
    body[5] = u8::try_from(args.len()).unwrap_or(u8::MAX);
    body[6] = class;
    body[7] = id;
    body[8..8 + args.len().min(80)].copy_from_slice(&args[..args.len().min(80)]);
    body[88] = body[2..88].iter().fold(0, |acc, b| acc ^ b);
    r
}

/// Take the LEDs from the controller's own effect (`true`), or hand them
/// back (`false`). The device forgets this when it loses power.
pub fn host_control(on: bool) -> Vec<u8> {
    control(0x00, 0x04, &[if on { 0x03 } else { 0x00 }, 0x00])
}

/// One frame: a report per port, LED 0 first, three bytes each in RGB.
/// A port shorter than [`LEDS`] is padded dark, and a longer one cut.
pub fn frame(ports: &[Vec<[u8; 3]>; PORTS]) -> Vec<Vec<u8>> {
    ports
        .iter()
        .enumerate()
        .map(|(port, leds)| {
            let mut r = vec![0u8; 1 + FRAME_LEN];
            // The last report of a frame shows it.
            r[1] = if port == PORTS - 1 { 0x84 } else { 0x04 };
            #[allow(clippy::cast_possible_truncation)]
            {
                r[2] = port as u8;
                r[3] = port as u8;
            }
            r[4] = 0x00;
            #[allow(clippy::cast_possible_truncation)]
            {
                r[5] = (LEDS - 1) as u8;
            }
            for (led, color) in leds.iter().take(LEDS).enumerate() {
                r[6 + led * 3..9 + led * 3].copy_from_slice(color);
            }
            r
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_control_report_matches_the_one_the_unit_answered() {
        let report = host_control(true);
        assert_eq!(report.len(), 91);
        assert_eq!(
            &report[1..10],
            &[0x00, 0x1F, 0x00, 0x00, 0x00, 0x02, 0x00, 0x04, 0x03]
        );
        // The checksum covers bytes 2 to 87 of the report body.
        assert_eq!(report[1 + 88], 0x02 ^ 0x04 ^ 0x03);
        assert_eq!(host_control(false)[1 + 8], 0x00);
    }

    #[test]
    fn a_frame_sends_every_port_and_flags_the_last() {
        let mut ports: [Vec<[u8; 3]>; PORTS] = Default::default();
        ports[3] = vec![[0, 255, 0]; 60];
        let reports = frame(&ports);
        assert_eq!(reports.len(), PORTS);
        assert!(reports.iter().all(|r| r.len() == 321));
        assert_eq!(&reports[3][1..6], &[0x04, 3, 3, 0x00, 0x4F]);
        assert_eq!(reports[5][1], 0x84);
        assert_eq!(&reports[3][6..9], &[0, 255, 0]);
        // LED 59 is the last one set; LED 60 is padding.
        assert_eq!(&reports[3][6 + 59 * 3..9 + 59 * 3], &[0, 255, 0]);
        assert_eq!(&reports[3][6 + 60 * 3..9 + 60 * 3], &[0, 0, 0]);
    }
}
