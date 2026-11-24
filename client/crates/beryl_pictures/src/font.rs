//! The small bitmap font that labels a film's frames: the digits and the letters of `ms`, each
//! five pixels wide and seven high. Text in the table is not drawn with it.

/// A glyph's rows from the top, each five bits with the leftmost pixel in bit 4.
fn glyph(character: char) -> [u8; 7] {
    match character {
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        'm' => [0, 0, 0b11010, 0b10101, 0b10101, 0b10101, 0b10101],
        's' => [0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
        _ => [0; 7],
    }
}

/// How many pixels wide `text` is at `scale`, with a one-pixel gap between glyphs.
pub fn width(text: &str, scale: u32) -> u32 {
    (text.chars().count() as u32 * 6).saturating_sub(1) * scale
}

/// How many pixels high a line of text is at `scale`.
pub fn height(scale: u32) -> u32 {
    7 * scale
}

/// Draws `text` white onto a black box of `pad` pixels around it, with the box's top left
/// corner at (`x`, `y`) in `pixels`, which are `stride` pixels wide RGBA rows. Whatever falls
/// outside the picture is not drawn.
pub fn label(pixels: &mut [u8], stride: u32, x: u32, y: u32, text: &str, scale: u32, pad: u32) {
    let rows = u32::try_from(pixels.len() / (stride as usize * 4)).unwrap_or(0);
    let mut put = |px: u32, py: u32, value: u8| {
        if px < stride && py < rows {
            let at = (py as usize * stride as usize + px as usize) * 4;
            pixels[at..at + 4].copy_from_slice(&[value, value, value, 255]);
        }
    };
    for py in 0..height(scale) + 2 * pad {
        for px in 0..width(text, scale) + 2 * pad {
            put(x + px, y + py, 0);
        }
    }
    for (index, character) in text.chars().enumerate() {
        for (row, bits) in glyph(character).into_iter().enumerate() {
            for column in 0..5u32 {
                if bits >> (4 - column) & 1 == 1 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            put(
                                x + pad + (index as u32 * 6 + column) * scale + dx,
                                y + pad + row as u32 * scale + dy,
                                255,
                            );
                        }
                    }
                }
            }
        }
    }
}
