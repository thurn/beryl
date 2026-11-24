//! Font weights, styled spans and inline symbols in shaped text.

use kit_text::text::fonts::Font;
use kit_text::text::{Block, Glyphs, Span, Text, TypeStyle};

const BODY: TypeStyle = TypeStyle {
    font: Font::OpenSans,
    weight: 400,
    italic: false,
    size: 20.0,
    line_height: 1.2,
    letter_spacing: 0.0,
};

#[test]
fn a_variable_font_is_drawn_at_any_weight() {
    let mut text = Text::new();
    let bold = TypeStyle {
        weight: 700,
        ..BODY
    };
    let (regular_width, _) = text.measure("Counterpoint", BODY, &[], None);
    let (bold_width, _) = text.measure("Counterpoint", bold, &[], None);
    assert!(
        bold_width > regular_width + 1.0,
        "{bold_width} vs {regular_width}"
    );
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

/// The lines and symbols of "Mix {A}{B}" in the body style, broken at `max_width`.
fn mix(text: &mut Text, max_width: Option<f32>) -> Block {
    text.block("Mix {A}{B}", BODY, &[], max_width).clone()
}

#[test]
fn icons_are_inline_boxes_an_em_wide_that_wrap_whole() {
    let mut text = Text::with_icons(Some(super::icon));
    let line = mix(&mut text, None);
    assert_eq!(line.lines.len(), 1);
    let [first, second] = line.symbols[..] else {
        panic!("two symbols, not {:?}", line.symbols);
    };
    let (word, _) = text.measure("Mix", BODY, &[], None);
    let em = BODY.size;
    for symbol in [first, second] {
        assert!((symbol.rect.width - em).abs() < 1e-3, "{symbol:?}");
    }
    assert!(first.rect.x >= word, "{first:?} after 'Mix' ({word})");
    assert!(
        (second.rect.x - (first.rect.x + em)).abs() < 0.01,
        "{second:?}"
    );
    assert_ne!(first.color, second.color);
    // A glyph on each symbol, besides those of 'Mix' and the space after it.
    assert_eq!(line.glyphs.len(), "Mix ".len() + 2);

    // Too narrow for 'Mix' and a symbol: the line breaks before the first symbol, and each
    // symbol lies whole inside one line.
    let narrow = word + em / 2.0;
    let wrapped = mix(&mut text, Some(narrow));
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
    let words = text.shaped_words("Mix {A}{B}");
    let joined: String = wrapped
        .lines
        .iter()
        .map(|line| line.words(&words))
        .collect();
    assert_eq!(joined, words);
    assert_eq!(wrapped.lines[0].words(&words).trim_end(), "Mix");
    for line in &wrapped.lines {
        assert!(line.baseline > line.rect.y && line.baseline < line.rect.y + line.rect.height);
    }
}

#[test]
fn plain_text_leaves_braces_in_the_line() {
    let mut text = Text::new();
    let words = "Use {braces}";
    let block = text.block(words, BODY, &[], None);
    assert!(block.symbols.is_empty());
    assert_eq!(block.lines[0].words(words), words);
}
