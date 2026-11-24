//! The desktop question dock, sized and placed in Beryl web's stage coordinates.

use game_table::profiles::Profile;
use glam::Vec2;
use kit_base::tokens::{color, shadow, size, type_style};
use kit_ui::element::{Anchor, Element, LayerDepth, TextAlign, text};
use kit_ui::layout::{DOCK_BOTTOM, DOCK_RIGHT};

/// The question wraps above the standard-width dock. Long questions extend to its left.
pub(super) fn question_title(asks: &str, title_length: usize) -> Element {
    // PromptBar.tsx uses the length of the session's title to choose the narrowly spaced type.
    let (style, extension) = if title_length > 40 {
        (
            type_style::LONG_QUESTION_TITLE,
            size::LONG_QUESTION_EXTENSION,
        )
    } else {
        (type_style::TITLE, 0.0)
    };
    text(asks)
        .style(style)
        .width(size::BUTTON_WIDTH + extension)
        .text_align(TextAlign::Center)
        .color(color::QUESTION_TITLE)
        .margin([0.0, 0.0, size::QUESTION_TITLE_GAP, -extension])
        .passes(true)
        // The two wider web shadows fill the glyph atlas at Retina scale and leave
        // no room for the question, buttons and reporter text.
        .drop_shadow(shadow::QUESTION_TITLE.to_vec())
}

/// Fits the desktop dock to the same stage as the web table. Its bottom right stays put as
/// the contents change size or wrap.
pub(super) fn fit_question_dock(mut dock: Element, profile: Profile) -> Element {
    dock.layer_depth = LayerDepth::AboveCards;
    if let Some((stage, scale)) = profile.fitted_stage() {
        let corner = Vec2::new(
            stage.x + stage.width - DOCK_RIGHT * scale,
            stage.y + stage.height - DOCK_BOTTOM * scale,
        );
        dock.anchor = Some(Anchor::Point(corner, Vec2::ONE));
        dock.style.inset = [None; 4];
        dock.style.scale = scale;
        dock.style.scale_origin = Vec2::ONE;
    }
    dock
}
