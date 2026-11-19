//! The design tokens the build script writes from `data/design-tokens.json`, against the CSS
//! values the web interface draws with.

use beryl_scene::element::Shadow;
use beryl_scene::text::fonts::Font;
use beryl_scene::tokens::{color, shadow, size, space, type_style};

#[test]
fn colors_are_the_css_values() {
    // `--surface: #211f1c`.
    assert_eq!(color::SURFACE, [0x21, 0x1f, 0x1c, 0xff]);
    // `--text-faint: rgb(242 236 225 / 38%)`, the alpha rounded from 96.9.
    assert_eq!(color::TEXT_FAINT, [242, 236, 225, 97]);
    // `--playable: rgb(var(--playable-rgb))`, with `--playable-rgb: 74 222 108`.
    assert_eq!(color::PLAYABLE, [74, 222, 108, 0xff]);
    // The flat layer's panel: `--surface` with the alpha `flat.color.PANEL` gives it.
    assert_eq!(color::PANEL, [0x21, 0x1f, 0x1c, 0xe0]);
    // The flat layer's button: `--surface-raised`.
    assert_eq!(color::BUTTON, color::SURFACE_RAISED);
}

#[test]
fn flat_values_follow_the_tokens_they_name() {
    // `--button-font: 24px`.
    exactly(type_style::BUTTON.size, 24.0);
    assert_eq!(type_style::BUTTON.font, Font::CrimsonPro);
    assert_eq!(type_style::BUTTON.weight, 700);
    // `--button-gap: 10px` and `--button-height: 48px`.
    exactly(space::M, 10.0);
    exactly(size::BUTTON_HEIGHT, 48.0);
    assert!(type_style::ALL.contains(&type_style::TITLE));
}

#[test]
fn shadows_are_the_css_lists() {
    // `.tip`: `0 3px 8px rgb(0 0 0 / 50%)`, the alpha rounded from 127.5.
    assert_eq!(shadow::TIP, [Shadow::new(0.0, 3.0, 8.0, [0, 0, 0, 128])]);
    // `flat.shadow.LIFTED` names `--shadow-lifted`: two layers.
    assert_eq!(shadow::LIFTED.len(), 2);
    // An inset ring: `inset 0 0 0 1px rgb(214 192 150 / 16%)`.
    assert_eq!(
        shadow::ICON_BUTTON[0],
        Shadow::new(0.0, 0.0, 0.0, [214, 192, 150, 41])
            .spread(1.0)
            .inset()
    );
}

/// Asserts that `a` is exactly `b`: a token's pixels are written as they are, not computed.
fn exactly(a: f32, b: f32) {
    assert_eq!(a.to_bits(), b.to_bits(), "{a} is not {b}");
}
