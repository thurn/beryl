//! How an ability plate looks over the card it comes from: one look for each frame family
//! (each of the five colors, multicolored, colorless and land), so that a plate over a blue
//! card is edged and tinted blue, and one over a land is tinted like a land's frame.

use tiny_skia::{FillRule, Mask, Path, PremultipliedColorU8, Transform};

use super::paint::{Canvas, Rgb, mix};
use super::{Palette, frame_key, palette};
use crate::faces::FaceText;

/// The paper the plate's tints are thinned toward, as the drawn frame's rules box thins its
/// bars' color.
const PARCHMENT: Rgb = [0xf8, 0xf5, 0xec];

/// The frame family of a face, which picks its plate's look: a color's letter, `m` for
/// multicolored, `c` for colorless and `l` for a colorless land. A colorless land with one basic
/// land type takes that type's color, as the drawn frame does, and otherwise the color of the
/// mana its rules text adds (Lonely Sandbar's "{T}: Add {U}." makes it blue), as printed land
/// frames do. A land that adds two or more colors is multicolored.
pub(crate) fn family(colors: &[String], text: &FaceText) -> char {
    let key = frame_key(colors, &text.type_line);
    if key != 'c' || !text.type_line.contains("Land") {
        return key;
    }
    let mut made = ['w', 'u', 'b', 'r', 'g'].map(|letter| (letter, false));
    for line in text.rules.as_deref().unwrap_or_default().lines() {
        let Some(at) = line.find("Add ") else {
            continue;
        };
        let added = &line[at..];
        let added = added.split('.').next().unwrap_or(added);
        if added.contains("any color") {
            return 'm';
        }
        for (letter, seen) in &mut made {
            let symbol = format!("{{{}}}", letter.to_ascii_uppercase());
            *seen |= added.contains(&symbol);
        }
    }
    let mut colors = made.iter().filter(|(_, seen)| *seen);
    match (colors.next(), colors.next()) {
        (None, _) => 'l',
        (Some((letter, _)), None) => *letter,
        _ => 'm',
    }
}

/// The colors and texture of one family's plate.
#[derive(Clone, Copy, Debug)]
pub(super) struct Look {
    /// The dark line around the plate.
    pub(super) rim: Rgb,
    /// The band in the frame's color inside that line.
    pub(super) band: Rgb,
    /// The paper's color at its top and at its bottom.
    pub(super) paper: [Rgb; 2],
    /// The thin line just inside the paper's edge.
    pub(super) edge: Rgb,
    /// The edge of the label's tab.
    pub(super) tab: Rgb,
    /// How strong the paper's grain is, and how strong its broad streaks are, in levels of
    /// each channel above and below the paper's color.
    pub(super) texture: (f32, f32),
    /// Whether the streaks run across the plate, like brushed metal or water, rather than
    /// winding like stone or leaves.
    pub(super) level: bool,
}

/// The look of a plate over a face of `family` ([`family`]). The five colors and multicolored
/// take the drawn frame's palettes, so a plate matches the drawn frame's rim and bars.
pub(super) fn look(family: char) -> Look {
    let frame = match family {
        // Silver, like an artifact's frame.
        'c' => Palette {
            rim: [0xa7, 0xad, 0xb3],
            rim_hi: [0xdc, 0xe0, 0xe3],
            rim_lo: [0x34, 0x39, 0x3f],
            bar: [0xc4, 0xc8, 0xcc],
            bar_hi: [0xe6, 0xe8, 0xea],
        },
        // Brown and tan, like a colorless land's frame.
        'l' => Palette {
            rim: [0x9c, 0x7a, 0x50],
            rim_hi: [0xd6, 0xbb, 0x8f],
            rim_lo: [0x3b, 0x2c, 0x1c],
            bar: [0xcf, 0xbb, 0x9b],
            bar_hi: [0xea, 0xde, 0xc8],
        },
        key => palette(key),
    };
    let (grain, streak, level) = match family {
        'w' => (3.0, 3.0, false),
        // Water and brushed metal run level across the plate.
        'u' | 'c' => (3.0, 6.0, true),
        'b' => (5.0, 7.0, false),
        'l' => (6.0, 6.0, false),
        _ => (4.0, 5.0, false),
    };
    Look {
        rim: frame.rim_lo,
        band: frame.rim,
        paper: [
            mix(frame.bar_hi, 0.6, PARCHMENT),
            mix(frame.bar, 0.7, PARCHMENT),
        ],
        edge: frame.rim_lo,
        tab: if family == 'm' {
            [0xc9, 0xa3, 0x5c]
        } else {
            frame.rim_hi
        },
        texture: (grain, streak),
        level,
    }
}

/// Lightens and darkens the pixels inside `shape` by the paper's grain and streaks, so that it
/// reads as printed card stock rather than flat color. `em` scales the streaks with the face.
pub(super) fn texture(canvas: &mut Canvas, shape: &Path, look: &Look, em: f32) {
    let (width, height) = (canvas.pixmap.width(), canvas.pixmap.height());
    let Some(mut mask) = Mask::new(width, height) else {
        return;
    };
    mask.fill_path(shape, FillRule::Winding, true, Transform::identity());
    let (grain, streak) = look.texture;
    let wave = 1.0 / em.max(1.0);
    let width = width as usize;
    let pixels = canvas.pixmap.pixels_mut();
    for (at, (pixel, &cover)) in pixels.iter_mut().zip(mask.data()).enumerate() {
        if cover == 0 {
            continue;
        }
        let (x, y) = ((at % width) as f32, (at / width) as f32);
        let (u, v) = (x * wave, y * wave);
        let streaks = if look.level {
            (2.1 * v + 0.6 * (0.7 * u).sin()).sin()
        } else {
            (0.9 * u + 1.3 * (0.8 * v + 0.5 * (1.1 * u).sin()).sin()).sin()
        };
        let noise = f32::from((hash(at as u32) & 0xff) as u8) / 127.5 - 1.0;
        let shift = (grain * noise + streak * streaks) * f32::from(cover) / 255.0;
        let alpha = pixel.alpha();
        let channel = |value: u8| {
            (f32::from(value) + shift)
                .round()
                .clamp(0.0, f32::from(alpha)) as u8
        };
        if let Some(shifted) = PremultipliedColorU8::from_rgba(
            channel(pixel.red()),
            channel(pixel.green()),
            channel(pixel.blue()),
            alpha,
        ) {
            *pixel = shifted;
        }
    }
}

/// A well-mixed 32-bit hash of a pixel's index, for the grain.
fn hash(mut value: u32) -> u32 {
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}
