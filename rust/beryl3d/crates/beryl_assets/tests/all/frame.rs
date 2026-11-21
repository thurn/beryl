//! Beryl's drawn card frame: the picture of a face whose scan has not been downloaded, at both
//! face sizes.

use beryl_assets::{FaceText, MatchFace, Plate, dimensions, frame, frames_drawn_on_this_thread};
use kit_base::atlas::{FaceId, FaceSize};

const SIZES: [FaceSize; 2] = [FaceSize::Board, FaceSize::Full];

/// A face with `text`'s words and no card behind it.
fn made_up(name: &str, colors: &[&str], text: FaceText) -> MatchFace {
    MatchFace {
        scan: String::new(),
        key: String::new(),
        name: name.to_owned(),
        colors: colors.iter().map(|&c| c.to_owned()).collect(),
        text,
        plate: None,
    }
}

/// The pixels of a face `width` pixels wide whose channels are all below `dark`, in the rows
/// and columns given as shares of the face's height and width.
fn dark_pixels(
    pixels: &[u8],
    width: u32,
    rows: (f32, f32),
    columns: (f32, f32),
    dark: u8,
) -> usize {
    let height = pixels.len() as u32 / 4 / width;
    let span = |(from, to): (f32, f32), length: u32| {
        (from * length as f32) as u32..(to * length as f32) as u32
    };
    let mut count = 0;
    for y in span(rows, height) {
        for x in span(columns, width) {
            let at = ((y * width + x) * 4) as usize;
            if pixels[at..at + 3].iter().all(|&c| c < dark) {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn counterspell_is_drawn_at_both_sizes_with_its_name_inked_in_the_name_bar() {
    let counterspell = made_up(
        "Counterspell",
        &["U"],
        FaceText {
            mana_cost: Some("{U}{U}".to_owned()),
            type_line: "Instant".to_owned(),
            rules: Some("Counter target spell.".to_owned()),
            ..FaceText::default()
        },
    );
    assert_eq!(counterspell.text.mana_cost.as_deref(), Some("{U}{U}"));
    for size in SIZES {
        let pixels = frame(&counterspell, size);
        let (width, height) = dimensions(size);
        assert_eq!(pixels.len(), (width * height * 4) as usize, "{size:?}");
        // The name bar's middle third, from the rim's left edge: about a tenth of the face from
        // its top on both frames. Its color is pale blue, its name nearly black.
        let rows = match size {
            FaceSize::Board => (0.07, 0.12),
            FaceSize::Full => (0.075, 0.12),
        };
        let inked = dark_pixels(&pixels, width, rows, (0.12, 0.45), 60);
        assert!(
            inked > 20,
            "{size:?}: {inked} pixels of ink in the name bar"
        );
        // The bar's right end, past the name and (on the full frame) the cost's left edge, holds
        // no ink.
        let bar = dark_pixels(&pixels, width, rows, (0.6, 0.7), 60);
        assert_eq!(bar, 0, "{size:?}");
    }
}

#[test]
fn a_long_rules_text_shrinks_to_fit_its_box() {
    let text = |rules: &str| FaceText {
        type_line: "Sorcery".to_owned(),
        rules: Some(rules.to_owned()),
        ..FaceText::default()
    };
    let long = ["Draw a card, then scry {2}."; 80].join(" ");
    assert!(long.split_whitespace().count() >= 400);
    let blank = frame(&made_up("Long", &["U"], text("")), FaceSize::Full);
    let full = frame(&made_up("Long", &["U"], text(&long)), FaceSize::Full);
    let (width, height) = dimensions(FaceSize::Full);
    let row = (width * 4) as usize;
    // The rules box ends a little above the card's foot (`card-full`'s padding, 0.84em, and the
    // rim's, 0.23em); every pixel below it is the same as with no text.
    let foot = (height as f32 - 1.2 * width as f32 / 12.0) as usize;
    assert_eq!(
        full[foot * row..],
        blank[foot * row..],
        "the text ran out of its box"
    );
    // The type bar above the box is untouched too.
    let top = (height as f32 * 0.55) as usize;
    assert_eq!(full[..top * row], blank[..top * row]);
    // And the box is full of words.
    let words = dark_pixels(&full, width, (0.62, 0.9), (0.1, 0.9), 90);
    assert!(words > 5_000, "{words} pixels of words");
}

#[test]
fn a_token_a_planeswalker_and_an_ability_plate_draw() {
    let token = made_up(
        "Spirit",
        &["W"],
        FaceText {
            type_line: "Token Creature — Spirit".to_owned(),
            rules: Some("Flying".to_owned()),
            power: Some("1".to_owned()),
            toughness: Some("1".to_owned()),
            token: true,
            ..FaceText::default()
        },
    );
    let walker = made_up(
        "Walker",
        &["W", "U"],
        FaceText {
            mana_cost: Some("{2}{W}{U}".to_owned()),
            type_line: "Legendary Planeswalker".to_owned(),
            rules: Some("+1: Draw a card.\n−3: Tap target creature.".to_owned()),
            loyalty: Some("4".to_owned()),
            ..FaceText::default()
        },
    );
    let mut plate = walker.clone();
    plate.plate = Some(Plate {
        source: FaceId(1),
        triggered: false,
        rules: "+1: Draw a card.".to_owned(),
    });
    for size in SIZES {
        let (width, height) = dimensions(size);
        for face in [&token, &walker, &plate] {
            assert_eq!(frame(face, size).len(), (width * height * 4) as usize);
        }
        // The token's body is pewter where a card's is black: its foot, below the rim.
        let token = frame(&token, size);
        let at = (((height - 4) * width + width / 2) * 4) as usize;
        assert!(
            token[at] > 90,
            "{size:?}: the token's foot is {:?}",
            &token[at..at + 3]
        );
    }
    assert!(frames_drawn_on_this_thread() >= 6);
}
