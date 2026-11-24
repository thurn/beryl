//! The web's serif face, `--font-serif` (`"Times New Roman", Times, serif`): Times New Roman
//! where the build found the system's copy, and Crimson Pro in its place elsewhere.

use kit_text::text::fonts::{ALL, Font};
use kit_text::text::{Text, TypeStyle};

/// The serif style at `weight`, 24 points on one line.
fn serif(weight: u16) -> TypeStyle {
    TypeStyle {
        font: Font::Serif,
        weight,
        italic: false,
        size: 24.0,
        line_height: 1.0,
        letter_spacing: 0.0,
    }
}

#[test]
fn the_serif_face_uses_the_installed_times_new_roman() {
    let times = match std::env::consts::OS {
        "macos" => [
            "Times New Roman.ttf",
            "Times New Roman Bold.ttf",
            "Times New Roman Italic.ttf",
            "Times New Roman Bold Italic.ttf",
        ]
        .iter()
        .all(|file| {
            std::path::Path::new("/System/Library/Fonts/Supplemental")
                .join(file)
                .is_file()
        }),
        "windows" => std::env::var_os("WINDIR").is_some_and(|root| {
            let folder = std::path::PathBuf::from(root).join("Fonts");
            ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"]
                .iter()
                .all(|file| folder.join(file).is_file())
        }),
        _ => false,
    };
    let expected = if times {
        "Times New Roman"
    } else {
        "Crimson Pro"
    };
    assert_eq!(Font::Serif.family(), expected);
    // Only Times New Roman has a separate bold file.
    let (regular, bold) = (
        &ALL[usize::from(Font::Serif.face(400, false))],
        &ALL[usize::from(Font::Serif.face(700, false))],
    );
    assert_eq!(std::ptr::eq(regular.data, bold.data), !times);
    assert_eq!(Font::Serif.face(600, false), Font::Serif.face(700, false));
    assert_eq!(Font::Serif.face(500, false), Font::Serif as u8);
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

#[test]
fn a_serif_italic_draws_from_its_own_file_when_available() {
    // The web sets its captions in italic Times New Roman, which is narrower than upright (much
    // narrower at 400, a little at 700). Each Times New Roman italic shapes and draws from
    // its own file. With Crimson Pro both styles use the same file and width.
    let italic = |weight| TypeStyle {
        italic: true,
        ..serif(weight)
    };
    let times = Font::Serif.family() == "Times New Roman";
    for weight in [400, 700] {
        let (upright, slanted) = (
            Font::Serif.face(weight, false),
            Font::Serif.face(weight, true),
        );
        assert_eq!(upright != slanted, times, "weight {weight}");
        if times {
            let file = ALL[usize::from(slanted)].file;
            let expected = if cfg!(windows) {
                if weight == 700 {
                    "timesbi.ttf"
                } else {
                    "timesi.ttf"
                }
            } else {
                "Italic.ttf"
            };
            assert!(file.ends_with(expected), "{file}");
        }
        let mut text = Text::new();
        let words = "Serif \u{b7} titles and actions";
        let (upright, slanted) = (
            text.measure(words, serif(weight), &[], None).0,
            text.measure(words, italic(weight), &[], None).0,
        );
        if times {
            assert!(
                slanted + 0.1 < upright,
                "italic {slanted}, upright {upright}"
            );
        } else {
            assert!(
                (slanted - upright).abs() < 0.01,
                "italic {slanted}, upright {upright}"
            );
        }
    }
    // Only the serif face has italic files.
    assert_eq!(Font::OpenSans.face(400, true), Font::OpenSans as u8);
}
