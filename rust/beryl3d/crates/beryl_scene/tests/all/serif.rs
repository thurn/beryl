//! The web's serif face, `--font-serif` (`"Times New Roman", Times, serif`): Times New Roman
//! where the build found the Mac's copy, and Crimson Pro in its place elsewhere.

use beryl_scene::text::fonts::{ALL, Font};
use beryl_scene::text::{Text, TypeStyle};

/// The serif style at `weight`, 24 points on one line.
fn serif(weight: u16) -> TypeStyle {
    TypeStyle {
        font: Font::Serif,
        weight,
        size: 24.0,
        line_height: 1.0,
        letter_spacing: 0.0,
    }
}

#[test]
fn the_serif_face_is_times_new_roman_on_a_mac() {
    let on_mac = cfg!(target_os = "macos")
        && std::path::Path::new("/System/Library/Fonts/Supplemental/Times New Roman.ttf").is_file();
    let expected = if on_mac {
        "Times New Roman"
    } else {
        "Crimson Pro"
    };
    assert_eq!(Font::Serif.family(), expected);
    // Only Times New Roman has a separate bold file.
    let (regular, bold) = (
        &ALL[usize::from(Font::Serif.face(400))],
        &ALL[usize::from(Font::Serif.face(700))],
    );
    assert_eq!(std::ptr::eq(regular.data, bold.data), !on_mac);
    assert_eq!(Font::Serif.face(600), Font::Serif.face(700));
    assert_eq!(Font::Serif.face(500), Font::Serif as u8);
}

#[test]
fn a_serif_weight_draws_from_the_file_its_glyph_keys_name() {
    // The rasterizer draws a glyph from the file `Font::face` names, so parley must shape each
    // weight with that same file: 600 with the bold, as CSS matches it when only 400 and 700
    // exist. Times New Roman's files are not variable, so text shaped from one file is as wide
    // at any weight it serves, and the two files' widths differ.
    let mut text = Text::new();
    let mut width = |weight| text.measure("New Game", serif(weight), &[], None).0;
    let (regular, semibold, bold) = (width(400), width(600), width(700));
    if Font::Serif.family() == "Times New Roman" {
        assert!(
            (semibold - bold).abs() < 0.01,
            "600 is {semibold} wide, 700 {bold}"
        );
        assert!(bold > regular + 1.0, "700 is {bold} wide, 400 {regular}");
    }
}
