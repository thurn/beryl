//! The generated pictures that fill the board-size face array until a match's real faces
//! arrive: a placeholder face per card name and a placeholder back, already written as every
//! mipmap level of the array's format.
//!
//! The asset workers draw them ([`crate::Preparer::placeholders`]), because a match of a hundred
//! faces takes tens of milliseconds, and the frame thread only copies the levels into the
//! array. No texture encoder runs: the placeholder is made of shapes on the 4 by 4 pixel grid of
//! a compressed block, and each block is written as one solid color, which every format can say
//! in a few fixed bits. [`placeholders_drawn_on_this_thread`] counts the pictures drawn, so that
//! a test can show the frame thread drew none.

use std::cell::Cell;

use kit_base::atlas::FaceSize;

use crate::encode::{Format, Layer, dimensions};

/// The side of a compressed block, in pixels, and its size in bytes in all three formats.
const BLOCK: u32 = 4;
const BLOCK_BYTES: usize = 16;

thread_local! {
    static DRAWN: Cell<u64> = const { Cell::new(0) };
}

/// How many placeholder pictures this thread has drawn. The asset workers draw a match's, so on
/// the thread that draws frames this stays zero once a match's faces are listed.
#[must_use]
pub fn placeholders_drawn_on_this_thread() -> u64 {
    DRAWN.with(Cell::get)
}

/// The board-size placeholders of one match, in `FaceId` order, all in `format`: layer 0, the
/// card back's, shows the placeholder back and each other layer its name's placeholder face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placeholders {
    pub format: Format,
    pub layers: Vec<Layer>,
}

impl Placeholders {
    /// Draws the placeholders of faces named `names` (the first name, the card back's, is not
    /// drawn) in `format`, with at least the back's layer.
    #[must_use]
    pub fn draw<S: AsRef<str>>(names: &[S], format: Format) -> Self {
        Self::draw_from(names, 0, format)
    }

    /// Draws the placeholders of the faces named `names[from..]`, for faces added to a match
    /// whose earlier faces already have layers: layer `i` of the result is face `from + i`'s.
    /// With `from` 0 this is [`Self::draw`].
    #[must_use]
    pub fn draw_from<S: AsRef<str>>(names: &[S], from: usize, format: Format) -> Self {
        let size = dimensions(FaceSize::Board);
        let count = names.len().max(1);
        let layers = (from..count)
            .map(|index| {
                let pixels = if index == 0 {
                    back(size)
                } else {
                    face(names[index].as_ref(), size)
                };
                solid_layer(&pixels, size, format)
            })
            .collect();
        Self { format, layers }
    }
}

/// One mipmap level's shape: its size in texels as a copy names it (whole blocks when
/// compressed), and its bytes per row of texels or blocks and number of such rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: u32,
    pub rows: u32,
}

impl Level {
    /// Level `level` of a layer in `format` whose largest level is `width` by `height`.
    #[must_use]
    pub fn of(format: Format, (width, height): (u32, u32), level: u32) -> Self {
        let width = (width >> level).max(1);
        let height = (height >> level).max(1);
        if format == Format::Rgba8 {
            return Self {
                width,
                height,
                bytes_per_row: width * 4,
                rows: height,
            };
        }
        let (across, down) = (width.div_ceil(BLOCK), height.div_ceil(BLOCK));
        Self {
            width: across * BLOCK,
            height: down * BLOCK,
            bytes_per_row: across * BLOCK_BYTES as u32,
            rows: down,
        }
    }

    /// How many mipmap levels a layer `width` by `height` has, down to 1 by 1.
    #[must_use]
    pub fn count((width, height): (u32, u32)) -> u32 {
        u32::BITS - width.max(height).leading_zeros()
    }

    #[must_use]
    pub fn bytes(&self) -> usize {
        (self.bytes_per_row * self.rows) as usize
    }
}

/// Every mipmap level of the placeholder `pixels`, RGBA `size` texels large, in `format`.
#[must_use]
pub fn solid_layer(pixels: &[u8], size: (u32, u32), format: Format) -> Layer {
    Layer {
        format,
        width: size.0,
        height: size.1,
        levels: (0..Level::count(size))
            .map(|level| solid_level(pixels, size, format, level))
            .collect(),
    }
}

/// FNV-1a, a small stable hash: the same name gives the same face on every machine.
fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// A picture `width` by `height` whose pixel at (x, y) is `color(x, y)`.
fn picture((width, height): (u32, u32), color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
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
/// 4, so that each 4 by 4 block of a compressed face holds one color (`solid_level`).
const FRAME: u32 = 8;
const STRIPE: u32 = 48;

fn in_frame(x: u32, y: u32, (width, height): (u32, u32)) -> bool {
    x < FRAME || y < FRAME || x >= width - FRAME || y >= height - FRAME
}

/// The placeholder face for a card named `name`, `size` pixels large: a flat color with a darker
/// frame and a lighter stripe, the color and the stripe's place both chosen by a hash of the
/// name, so that two cards are told apart in a picture.
#[must_use]
fn face(name: &str, size: (u32, u32)) -> Vec<u8> {
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
#[must_use]
fn back(size: (u32, u32)) -> Vec<u8> {
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

/// Level `level` of the placeholder `pixels`, RGBA `size` texels large, in `format`: each texel,
/// or each block of a compressed format, takes the color of the full-size pixel at its centre.
fn solid_level(pixels: &[u8], size: (u32, u32), format: Format, level: u32) -> Vec<u8> {
    let shape = Level::of(format, size, level);
    let step = if format == Format::Rgba8 { 1 } else { BLOCK };
    let mut bytes = Vec::with_capacity(shape.bytes());
    for y in (0..shape.height).step_by(step as usize) {
        for x in (0..shape.width).step_by(step as usize) {
            let centre =
                |at: u32, side: u32| ((at << level) + ((step << level) >> 1)).min(side - 1);
            let i = ((centre(y, size.1) * size.0 + centre(x, size.0)) * 4) as usize;
            let rgb = [pixels[i], pixels[i + 1], pixels[i + 2]];
            match format {
                Format::Rgba8 => bytes.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]),
                _ => bytes.extend_from_slice(&solid_block(format, rgb)),
            }
        }
    }
    bytes
}

/// One opaque 4 by 4 block of the single color `rgb` in a compressed format.
fn solid_block(format: Format, [r, g, b]: [u8; 3]) -> [u8; BLOCK_BYTES] {
    let mut block = [0; BLOCK_BYTES];
    match format {
        // A void-extent block (ASTC specification, "Void-Extent Blocks"): block mode 0x1FC,
        // LDR, the two reserved bits set and every extent coordinate all ones, then the color as
        // four 16-bit UNORM values.
        Format::Astc4x4 => {
            block[..8].copy_from_slice(&[0xFC, 0xFD, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
            for (at, value) in [r, g, b, 255].into_iter().enumerate() {
                let unorm = u16::from(value) * 257;
                block[8 + at * 2..10 + at * 2].copy_from_slice(&unorm.to_le_bytes());
            }
        }
        // EAC alpha, then an ETC1 block in individual mode (Khronos Data Format, "ETC2"). The
        // alpha's base is 255, with multiplier 1 and table 0, and every pixel index 4 adds 2,
        // which stays 255. The color is 4 bits a channel for both halves, and every pixel index
        // 0 adds table 0's smallest modifier, 2.
        Format::Etc2Rgba8 => {
            block[..8].copy_from_slice(&[255, 0x10, 0x92, 0x49, 0x24, 0x92, 0x49, 0x24]);
            for (at, value) in [r, g, b].into_iter().enumerate() {
                let four = ((u16::from(value.saturating_sub(2)) * 15 + 127) / 255) as u8;
                block[8 + at] = four << 4 | four;
            }
        }
        // BC7 mode 6 (Khronos Data Format, "BC7"): both endpoints the same color in 7 bits a
        // channel with a shared low bit of 1, and every index 0.
        Format::Bc7 => {
            let mut bits: u128 = 1 << 6;
            let mut at = 7;
            for value in [r, r, g, g, b, b, 255, 255] {
                bits |= u128::from(value >> 1) << at;
                at += 7;
            }
            bits |= 0b11 << 63;
            block = bits.to_le_bytes();
        }
        Format::Rgba8 => unreachable!("an uncompressed format has no blocks"),
    }
    block
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

    #[test]
    fn every_level_has_its_shape_s_bytes() {
        for format in [
            Format::Rgba8,
            Format::Astc4x4,
            Format::Etc2Rgba8,
            Format::Bc7,
        ] {
            let placeholders = Placeholders::draw(&["", "Island"], format);
            assert_eq!(placeholders.layers.len(), 2);
            let size = dimensions(FaceSize::Board);
            for layer in &placeholders.layers {
                assert_eq!(layer.levels.len() as u32, Level::count(size));
                for (level, bytes) in (0..).zip(&layer.levels) {
                    assert_eq!(bytes.len(), Level::of(format, size, level).bytes());
                }
            }
        }
    }
}
