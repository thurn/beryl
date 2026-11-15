//! The texture arrays a record samples (docs/beryl-3d.md, "The frame": three texture arrays),
//! and the generated pictures that fill them until real card faces and atlas pages are made:
//! a placeholder face per card name, a placeholder back, and the soft disc a glow draws.

use beryl_scene::frame::Sprite;

/// The size of a placeholder face. The shader samples a face by its texture coordinates, so a
/// face of any size fills the card.
pub const PLACEHOLDER_WIDTH: u32 = 128;
pub const PLACEHOLDER_HEIGHT: u32 = 180;
/// The layer of the face arrays that holds the placeholder card back; each placeholder face
/// for a name follows it.
pub const BACK_LAYER: u16 = 0;
/// The size of the placeholder atlas's one page.
const ATLAS_SIZE: u32 = 128;
/// The soft disc that a glow draws: the whole of the placeholder atlas's first page.
pub const GLOW_SPRITE: Sprite = Sprite {
    page: 0,
    rect: [0, 0, ATLAS_SIZE as u16, ATLAS_SIZE as u16],
};

/// The pictures of one texture array: every layer is `width` by `height` RGBA pixels, not
/// premultiplied, stored as sRGB values.
pub struct Layers {
    pub width: u32,
    pub height: u32,
    pub pictures: Vec<Vec<u8>>,
}

/// The placeholder card back in layer 0, and for each of `names` in order a placeholder face:
/// a flat color with a darker frame and a stripe, the color and the stripe's place both chosen
/// by a hash of the name, so that two cards are told apart in a picture.
#[must_use]
pub fn placeholder_faces(names: &[&str]) -> Layers {
    let mut layers = vec![back()];
    layers.extend(names.iter().map(|name| face(name)));
    Layers {
        width: PLACEHOLDER_WIDTH,
        height: PLACEHOLDER_HEIGHT,
        pictures: layers,
    }
}

/// The placeholder atlas: one page holding the soft disc of `GLOW_SPRITE`, white with its
/// opacity falling from the centre to nothing at the edge.
#[must_use]
pub fn placeholder_atlas() -> Layers {
    let size = ATLAS_SIZE;
    let mut page = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
            let distance = (dx * dx + dy * dy).sqrt().min(1.0);
            let alpha = (1.0 - distance) * (1.0 - distance);
            page.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    Layers {
        width: size,
        height: size,
        pictures: vec![page],
    }
}

/// FNV-1a, a small stable hash: the same name gives the same face on every machine.
fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// A picture of the placeholder size whose pixel at (x, y) is `color(x, y)`.
fn picture(color: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((PLACEHOLDER_WIDTH * PLACEHOLDER_HEIGHT * 4) as usize);
    for y in 0..PLACEHOLDER_HEIGHT {
        for x in 0..PLACEHOLDER_WIDTH {
            let [r, g, b] = color(x, y);
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    pixels
}

/// How many pixels wide a placeholder's frame is.
const FRAME: u32 = 6;

fn in_frame(x: u32, y: u32) -> bool {
    x < FRAME || y < FRAME || x >= PLACEHOLDER_WIDTH - FRAME || y >= PLACEHOLDER_HEIGHT - FRAME
}

fn face(name: &str) -> Vec<u8> {
    let hash = hash(name);
    let [r, g, b, s] = hash.to_le_bytes();
    // Kept between 64 and 223, so the frame is darker and the stripe lighter.
    let color = [r, g, b].map(|c| 64 + c % 160);
    let frame = color.map(|c| c / 3);
    let stripe = color.map(|c| c.saturating_add(96));
    let stripe_top = FRAME + u32::from(s) % (PLACEHOLDER_HEIGHT - 2 * FRAME - 24);
    picture(|x, y| {
        if in_frame(x, y) {
            frame
        } else if (stripe_top..stripe_top + 24).contains(&y) {
            stripe
        } else {
            color
        }
    })
}

fn back() -> Vec<u8> {
    picture(|x, y| {
        if in_frame(x, y) {
            [20, 14, 10]
        } else if (x + y) / 12 % 2 == 0 {
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
        let faces = placeholder_faces(&["Counterspell", "Island"]);
        assert_eq!(faces.pictures.len(), 3);
        assert_ne!(faces.pictures[1], faces.pictures[2]);
        assert_eq!(
            faces.pictures[1].len(),
            (PLACEHOLDER_WIDTH * PLACEHOLDER_HEIGHT * 4) as usize
        );
    }
}
