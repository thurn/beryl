//! Text in the flat layer: styled spans, inline mana symbols, and glyphs drawn from their old
//! size while the display's scale changes.

use beryl_scene::agent::checks::text_fits;
use beryl_scene::element::Span;
use beryl_scene::frame::{Frame, Material, Role};
use beryl_scene::input::{AssetReady, GlyphCell, Input, Insets, Size};
use beryl_scene::text::fonts::Font;
use beryl_scene::text::{Block, Glyphs, Text, TypeStyle, shaped_words};
use glam::Vec2;

use crate::flat_layer::{client, deciding, prompt};

const BODY: TypeStyle = TypeStyle {
    font: Font::OpenSans,
    weight: 400,
    italic: false,
    size: 20.0,
    line_height: 1.2,
    letter_spacing: 0.0,
};

/// Draws frames from `now` and gives every glyph the client wants a cell in atlas page `page`,
/// as many pixels square as its size so that a cell drawn at another size shows by how much it
/// was scaled, until the client wants no more. Returns the last frame.
fn fill_sized(client: &mut beryl_scene::Client, page: u16, now: u64) -> Frame {
    let mut filled = false;
    for tick in now.. {
        client.frame(tick);
        let glyphs: Vec<GlyphCell> = client
            .wanted_glyphs()
            .map(|key| GlyphCell {
                key,
                rect: [0, 0, key.size, key.size],
                bearing: [0, -i16::try_from(key.size).unwrap()],
            })
            .collect();
        if glyphs.is_empty() {
            assert!(filled, "the client wants glyphs");
            return client.frame(tick + 1).clone();
        }
        filled = true;
        client.handle(Input::Assets(AssetReady::Glyphs { page, glyphs }));
    }
    unreachable!("the loop returns")
}

/// The glyph records of `frame`'s flat layer, each as its transform.
fn glyph_models(frame: &Frame) -> Vec<[f32; 12]> {
    (frame.draws[frame.layer as usize..].iter())
        .filter(|draw| draw.material == Material::Glyph as u8)
        .map(|draw| draw.model)
        .collect()
}

fn corners(frame: &Frame) -> Vec<[Vec2; 4]> {
    frame
        .resolved
        .iter()
        .map(|element| element.corners)
        .collect()
}

/// Whether two transforms differ by less than `by` in every number.
fn near(a: &[f32; 12], b: &[f32; 12], by: f32) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < by)
}

#[test]
fn a_bold_span_lays_out_its_glyphs_at_the_bold_weight() {
    let mut text = Text::new();
    let bold = TypeStyle {
        weight: 700,
        ..BODY
    };
    let spans = [Span {
        range: 4..16,
        style: bold,
        color: None,
    }];
    let block = text.block("Pay Counterspell", BODY, &spans, None).clone();
    let weight_of = |weight: u16| {
        (block.glyphs.iter())
            .filter(|glyph| block.run(glyph.run).style.weight == weight)
            .count()
    };
    assert_eq!(weight_of(700), "Counterspell".len());
    assert_eq!(weight_of(400), "Pay ".len());
    let (plain, _) = text.measure("Pay Counterspell", BODY, &[], None);
    assert!(block.width > plain + 1.0, "{} vs {plain}", block.width);
    // The glyphs are asked for at the bold weight.
    let mut glyphs = Glyphs::default();
    glyphs.want(&block, 2.0);
    assert!(glyphs.drain_wanted().any(|key| key.weight == 700));
}

/// The lines and symbols of "Pay {1}{U}" in the body style, broken at `max_width`.
fn pay(text: &mut Text, max_width: Option<f32>) -> Block {
    text.block("Pay {1}{U}", BODY, &[], max_width).clone()
}

#[test]
fn mana_symbols_are_inline_boxes_an_em_wide_that_wrap_whole() {
    let mut text = Text::new();
    let line = pay(&mut text, None);
    assert_eq!(line.lines.len(), 1);
    let [one, blue] = line.symbols[..] else {
        panic!("two symbols, not {:?}", line.symbols);
    };
    let (word, _) = text.measure("Pay", BODY, &[], None);
    let em = BODY.size;
    for symbol in [one, blue] {
        assert!((symbol.rect.width - em).abs() < 1e-3, "{symbol:?}");
    }
    assert!(one.rect.x >= word, "{one:?} after 'Pay' ({word})");
    assert!((blue.rect.x - (one.rect.x + em)).abs() < 0.01, "{blue:?}");
    assert_ne!(one.color, blue.color, "a blue symbol is not a generic one");
    // A glyph on each symbol, besides those of 'Pay' and the space after it.
    assert_eq!(line.glyphs.len(), "Pay ".len() + 2);

    // Too narrow for 'Pay' and a symbol: the line breaks before the first symbol, and each
    // symbol lies whole inside one line.
    let narrow = word + em / 2.0;
    let wrapped = pay(&mut text, Some(narrow));
    assert!(wrapped.lines.len() >= 2, "{:?}", wrapped.lines);
    let first = wrapped.symbols[0].rect;
    assert!(first.x.abs() < 0.01, "{first:?} starts a line");
    let top = wrapped.lines[0].rect;
    assert!(first.y >= top.y + top.height - 0.01);
    for symbol in &wrapped.symbols {
        let rect = symbol.rect;
        assert!(rect.x + rect.width <= narrow + 0.01, "{rect:?}");
        let middle = rect.y + rect.height / 2.0;
        assert!(
            (wrapped.lines.iter())
                .any(|line| middle >= line.rect.y && middle <= line.rect.y + line.rect.height),
            "{rect:?} in {:?}",
            wrapped.lines
        );
    }
    // The lines' words, end to end, are the text as it was shaped, each symbol one object
    // character, and each baseline lies inside its line.
    let words = shaped_words("Pay {1}{U}");
    let joined: String = wrapped
        .lines
        .iter()
        .map(|line| line.words(&words))
        .collect();
    assert_eq!(joined, words);
    assert_eq!(wrapped.lines[0].words(&words).trim_end(), "Pay");
    for line in &wrapped.lines {
        assert!(line.baseline > line.rect.y && line.baseline < line.rect.y + line.rect.height);
    }
}

#[test]
fn glyphs_at_a_new_scale_draw_from_the_old_cells_and_move_nothing() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let before = fill_sized(&mut client, 0, 0);
    let drawn = glyph_models(&before);
    assert!(!drawn.is_empty());

    let surface = |scale| Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale,
        safe_area: Insets::default(),
    };
    client.handle(surface(3.0));
    // Until the new page arrives, every glyph is drawn from its cell at the old size, scaled
    // to where and how large it was. A size in pixels is a whole number, so a glyph's size in
    // points may differ by up to a pixel's rounding at either scale.
    let scaled = client.frame(10).clone();
    let old_cells = glyph_models(&scaled);
    assert_eq!(old_cells.len(), drawn.len());
    assert!(drawn.iter().zip(&old_cells).all(|(a, b)| near(a, b, 0.5)));
    assert_eq!(corners(&scaled), corners(&before));

    let after = fill_sized(&mut client, 1, 11);
    assert_eq!(corners(&after), corners(&before), "no element moves");
    let new_cells = glyph_models(&after);
    assert_eq!(new_cells.len(), drawn.len());
    // The new cells are drawn exactly where and as large as the old ones were scaled to.
    assert!(
        old_cells
            .iter()
            .zip(&new_cells)
            .all(|(a, b)| near(a, b, 1e-3))
    );
    let pages: Vec<u16> = (after.draws[after.layer as usize..].iter())
        .filter(|draw| draw.material == Material::Glyph as u8)
        .map(|draw| draw.layer)
        .collect();
    assert!(pages.contains(&1), "the new page is drawn from: {pages:?}");
}

#[test]
fn a_title_with_mana_symbols_fits_its_box() {
    let mut client = client();
    let title = "Pay {2}{W}{U} or sacrifice a creature with power {X}";
    client.handle(deciding(prompt(title)));
    let frame = client.frame(0);
    let text = (frame.resolved.iter())
        .position(|element| element.role == Role::Text && frame.name(element.name) == title)
        .expect("the title is drawn");
    assert!(frame.lines.iter().any(|line| line.element as usize == text));
    assert!(text_fits(frame, &crate::flat_layer::context()).is_empty());
}
