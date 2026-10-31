// SPDX-License-Identifier: Apache-2.0
//! Keeps text that enters the program from carrying terminal escapes.

/// Replaces characters that steer a terminal in a string from outside the
/// program.
///
/// A serial number is whatever the USB descriptor says, argv is whatever the
/// caller put there, and both end up on the terminal. Without this, a device
/// or a calling script could hide an ANSI escape sequence in the text and
/// control what the terminal does with the rest of the line, or reorder what
/// the line appears to say.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || steers_layout(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// Characters outside the control category that still decide how a terminal
/// lays text out.
///
/// `char::is_control` is the Unicode `Cc` category, which covers the C0 and C1
/// escapes and stops there. The bidirectional overrides below are `Cf`: they
/// emit nothing themselves, and they make the rest of the line display in an
/// order it was not written in - enough for a serial number to be shown as one
/// that belongs to a different board. The zero width characters are here for
/// the same reason in the other direction: they let two different strings look
/// identical. A serial number has a use for none of them.
fn steers_layout(c: char) -> bool {
    matches!(
        c,
        '\u{00ad}'                 // soft hyphen
            | '\u{061c}'           // arabic letter mark
            | '\u{200b}'..='\u{200f}' // zero width space through right-to-left mark
            | '\u{202a}'..='\u{202e}' // bidi embeddings and overrides
            | '\u{2060}'..='\u{2064}' // word joiner and the invisible operators
            | '\u{2066}'..='\u{2069}' // bidi isolates
            | '\u{feff}'           // byte order mark
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_number_from_the_device_cannot_carry_terminal_escapes() {
        // What a hostile device could put into its USB descriptor.
        assert_eq!(sanitize("YK\x1b[2JEVIL"), "YK?[2JEVIL");
        assert_eq!(sanitize("YK\r\n\x07"), "YK???");
        assert_eq!(sanitize("YK\u{9b}31m"), "YK?31m");

        // An ordinary serial number is left alone.
        assert_eq!(sanitize("Y3N13808"), "Y3N13808");
        assert_eq!(sanitize("<unknown>"), "<unknown>");
    }

    #[test]
    fn a_serial_number_cannot_reorder_the_line_it_is_printed_on() {
        // Bidi overrides and isolates are not control characters, so
        // `is_control` alone lets them through. Displayed, they turn the rest
        // of the line around.
        assert_eq!(sanitize("Y3N\u{202e}80831"), "Y3N?80831");
        assert_eq!(sanitize("Y3N\u{202d}138"), "Y3N?138");
        assert_eq!(sanitize("Y3N\u{2066}138\u{2069}"), "Y3N?138?");
        assert_eq!(sanitize("Y3N\u{200f}138"), "Y3N?138");
        assert_eq!(sanitize("\u{061c}Y3N138"), "?Y3N138");
    }

    #[test]
    fn two_serial_numbers_cannot_be_made_to_look_alike() {
        // Zero width characters print as nothing, so without this a doctored
        // board could show the serial number of another one.
        assert_eq!(sanitize("Y3N\u{200b}13808"), "Y3N?13808");
        assert_eq!(sanitize("Y3N\u{feff}13808"), "Y3N?13808");
        assert_eq!(sanitize("Y3N\u{00ad}13808"), "Y3N?13808");
        assert_eq!(sanitize("Y3N\u{2060}13808"), "Y3N?13808");
    }

    #[test]
    fn an_argument_echoed_in_an_error_cannot_carry_terminal_escapes() {
        // What a calling script could pass and later see echoed on stderr.
        assert_eq!(sanitize("-\x1b[31mred"), "-?[31mred");
        assert_eq!(sanitize("5\x07"), "5?");

        // The replacement character from a lossy argv conversion is kept, so
        // the error message still shows where the bad byte was.
        assert_eq!(sanitize("YK\u{fffd}1"), "YK\u{fffd}1");
    }
}
