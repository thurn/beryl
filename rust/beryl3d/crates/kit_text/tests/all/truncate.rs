//! Text cut to its box with an ellipsis.

use kit_text::text::fonts::Font;
use kit_text::text::{Block, Text, Truncate, TypeStyle, shaped_words};

const BODY: TypeStyle = TypeStyle {
    font: Font::OpenSans,
    weight: 400,
    italic: false,
    size: 20.0,
    line_height: 1.2,
    letter_spacing: 0.0,
};

const LONG: &str = "Return target creature an opponent controls to its owner's hand";

/// The words each line of `block`, laid out from `source`, shows.
fn lines(block: &Block, source: &str) -> Vec<String> {
    let words = shaped_words(block.shown.as_deref().unwrap_or(source)).into_owned();
    (block.lines.iter())
        .map(|line| line.words(&words).trim_end().to_owned())
        .collect()
}

#[test]
fn a_truncated_line_fits_its_box_and_ends_with_the_ellipsis() {
    let mut text = Text::new();
    let block = text.truncated(LONG, BODY, &[], Some(160.0), Truncate::ELLIPSIS);
    assert_eq!(block.lines.len(), 1);
    assert!(block.width <= 160.0, "{}", block.width);
    let shown = block.shown.as_deref().expect("the text was cut");
    assert!(shown.ends_with('\u{2026}'), "{shown}");
    assert!(LONG.starts_with(shown.trim_end_matches('\u{2026}')), "{shown}");
    // Nothing longer would fit: one more word goes past the box.
    let (whole, _) = text.measure(LONG, BODY, &[], None);
    assert!(whole > 160.0);
}

#[test]
fn a_text_that_fits_is_not_cut() {
    let mut text = Text::new();
    let block = text.truncated("Island", BODY, &[], Some(160.0), Truncate::ELLIPSIS);
    assert_eq!(block.shown, None);
    assert_eq!(lines(&block, "Island"), ["Island"]);
}

#[test]
fn a_two_line_clamp_keeps_two_lines_and_cuts_the_second() {
    let mut text = Text::new();
    let block = text.truncated(LONG, BODY, &[], Some(200.0), Truncate::clamp(2));
    let shown = lines(&block, LONG);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(block.width <= 200.0, "{}", block.width);
    assert!(shown[1].ends_with('\u{2026}'), "{shown:?}");
    // The whole text takes more than two lines at this width.
    assert!(text.block(LONG, BODY, &[], Some(200.0)).lines.len() > 2);
}

#[test]
fn a_mana_symbol_is_cut_whole() {
    let mut text = Text::new();
    let block = text.truncated("Pay {1}{U}{U}{U}", BODY, &[], Some(70.0), Truncate::ELLIPSIS);
    let shown = block.shown.as_deref().expect("the text was cut");
    let kept = shown.trim_end_matches('\u{2026}');
    assert_eq!(kept.matches('{').count(), kept.matches('}').count(), "{shown}");
}
