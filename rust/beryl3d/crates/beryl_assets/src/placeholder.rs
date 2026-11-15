//! The picture of a face whose scan has not been downloaded: the card's name on a field of its
//! color, inside a dark frame. Beryl's own drawn card frame replaces it later.

use beryl_scene::input::FaceSize;
use beryl_scene::text::fonts::Font;
use swash::scale::{Render, ScaleContext, Source};
use swash::{FontRef, GlyphId};

use crate::encode::dimensions;

/// The frame's width and the text's size, as fractions of the face's width.
const FRAME: f32 = 0.04;
const TEXT: f32 = 0.1;
/// The name's ink color.
const INK: [u8; 3] = [16, 14, 12];

/// RGBA pixels at `size`: `name` set in Crimson Pro on a field colored by `colors` (Magic's
/// color letters), broken into lines at spaces to fit, and centered.
#[must_use]
pub fn placeholder(name: &str, colors: &[String], size: FaceSize) -> Vec<u8> {
    let (width, height) = dimensions(size);
    let field = field(colors);
    let frame = field.map(|c| c / 4);
    let border = (width as f32 * FRAME) as u32;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let edge = x < border || y < border || x >= width - border || y >= height - border;
            let [r, g, b] = if edge { frame } else { field };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let em = width as f32 * TEXT;
    write_name(&mut pixels, width, height, name, em, width - 4 * border);
    pixels
}

/// The field's color: the card's color, gold for several, gray for none.
fn field(colors: &[String]) -> [u8; 3] {
    match colors {
        [] => [176, 172, 166],
        [one] => match one.as_str() {
            "W" => [236, 230, 206],
            "U" => [120, 170, 214],
            "B" => [130, 120, 120],
            "R" => [222, 128, 102],
            "G" => [128, 186, 132],
            _ => [176, 172, 166],
        },
        _ => [214, 186, 110],
    }
}

/// Draws `name` centered in the picture, at `em` pixels, in lines no wider than `room`.
fn write_name(pixels: &mut [u8], width: u32, height: u32, name: &str, em: f32, room: u32) {
    let Some(font) = FontRef::from_index(Font::CrimsonPro.file().data, 0) else {
        return;
    };
    let charmap = font.charmap();
    let advances = font.glyph_metrics(&[]).scale(em);
    let glyphs = |text: &str| -> Vec<(GlyphId, f32)> {
        text.chars()
            .map(|c| {
                let glyph = charmap.map(c);
                (glyph, advances.advance_width(glyph))
            })
            .collect()
    };
    let wide = |text: &str| glyphs(text).iter().map(|(_, a)| a).sum::<f32>();
    let mut lines: Vec<String> = Vec::new();
    for word in name.split_whitespace() {
        match lines.last_mut() {
            Some(line) if wide(&format!("{line} {word}")) <= room as f32 => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_owned()),
        }
    }
    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font).size(em).hint(true).build();
    let render = Render::new(&[Source::Outline]);
    let line_height = em * 1.25;
    let top = (height as f32 - line_height * lines.len() as f32) / 2.0;
    for (row, line) in lines.iter().enumerate() {
        let baseline = top + line_height * row as f32 + em;
        let mut pen = (width as f32 - wide(line)) / 2.0;
        for (glyph, advance) in glyphs(line) {
            if let Some(mask) = render.render(&mut scaler, glyph) {
                let p = mask.placement;
                let left = pen.round() as i32 + p.left;
                let top = baseline.round() as i32 - p.top;
                blend(pixels, width, height, &mask.data, p.width, left, top);
            }
            pen += advance;
        }
    }
}

/// Inks the 8-bit coverage mask `mask`, `mask_width` wide, with its top left corner at
/// (`left`, `top`).
fn blend(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    mask: &[u8],
    mask_width: u32,
    left: i32,
    top: i32,
) {
    if mask_width == 0 {
        return;
    }
    for (i, &cover) in mask.iter().enumerate() {
        let x = left + (i as u32 % mask_width) as i32;
        let y = top + (i as u32 / mask_width) as i32;
        if cover == 0 || x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            continue;
        }
        let at = ((y as u32 * width + x as u32) * 4) as usize;
        for (channel, ink) in pixels[at..at + 3].iter_mut().zip(INK) {
            let mixed = (u32::from(*channel) * (255 - u32::from(cover))
                + u32::from(ink) * u32::from(cover))
                / 255;
            *channel = mixed as u8;
        }
    }
}
