use super::words;
use kit_base::tokens::{color, gradient, shadow, size, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Element, Justify, column, text};

/// The banner with the detail at `detail`: the gold title in capitals over the detail, on a
/// smoky shade that fades out toward its edges (`.step-banner`'s radial gradient).
pub(super) fn banner(detail: usize) -> Element {
    let (title, detail) = words(detail);
    let mut element = column()
        .name("step banner")
        .width(size::BANNER_WIDTH)
        .height(size::BANNER_HEIGHT)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .gap(kit_base::tokens::space::M)
        .background_image(vec![gradient::BANNER])
        // It takes no pointer, so the permanents under it stay at hand, as the web's
        // `.step-banner` has `pointer-events: none`.
        .passes(true)
        .child(
            text(&title.to_uppercase())
                .name(title)
                .style(type_style::BANNER_TITLE)
                .color(color::BANNER_TITLE)
                .drop_shadow(shadow::BANNER_TITLE.to_vec()),
        )
        .child(
            text(detail)
                .style(type_style::BANNER_DETAIL)
                .color(color::BANNER_DETAIL),
        );
    element.role = Role::Region;
    element
}
