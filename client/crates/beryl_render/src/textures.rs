//! The generated pictures that fill the card face arrays until real faces arrive: a
//! placeholder face per card name and a placeholder back.

/// The layer of the face arrays that holds the card back: the card back is always the first
/// face supplied by the host.
pub const BACK_LAYER: u16 = 0;

/// FNV-1a, a small stable hash: the same name gives the same face on every machine.
fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// A picture `width` by `height` whose pixel at (x, y) is `color(x, y)`.
fn picture((width, height): (u32, u32), color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = color(x, y);
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// How many pixels wide a placeholder's frame is, and how tall a face's stripe is: multiples of
/// 4, so that each 4 by 4 block of a compressed face holds one color (`faces::solid_level`).
const FRAME: u32 = 8;
const STRIPE: u32 = 48;

fn in_frame(x: u32, y: u32, (width, height): (u32, u32)) -> bool {
    x < FRAME || y < FRAME || x >= width - FRAME || y >= height - FRAME
}

/// The placeholder face for a card named `name`, `size` pixels large: a flat color with a darker
/// frame and a lighter stripe, the color and the stripe's place both chosen by a hash of the
/// name, so that two cards are told apart in a picture.
pub(crate) fn face(name: &str, size: (u32, u32)) -> Vec<u8> {
    let hash = hash(name);
    let [r, g, b, s] = hash.to_le_bytes();
    // Kept between 64 and 223, so the frame is darker and the stripe lighter.
    let color = [r, g, b].map(|c| 64 + c % 160);
    let frame = color.map(|c| c / 3);
    let stripe = color.map(|c| c.saturating_add(96));
    let room = (size.1 - 2 * FRAME - STRIPE) / 4;
    let stripe_top = FRAME + u32::from(s) % room * 4;
    picture(size, |x, y| {
        if in_frame(x, y, size) {
            frame
        } else if (stripe_top..stripe_top + STRIPE).contains(&y) {
            stripe
        } else {
            color
        }
    })
}

/// The placeholder card back, `size` pixels large: a frame around diagonal bands.
pub(crate) fn back(size: (u32, u32)) -> Vec<u8> {
    picture(size, |x, y| {
        if in_frame(x, y, size) {
            [20, 14, 10]
        } else if (x / 4 + y / 4) / 6 % 2 == 0 {
            [92, 58, 34]
        } else {
            [70, 42, 24]
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_of_different_names_differ() {
        let size = (256, 360);
        let island = face("Island", size);
        assert_ne!(island, face("Counterspell", size));
        assert_eq!(island.len(), 256 * 360 * 4);
    }
}
