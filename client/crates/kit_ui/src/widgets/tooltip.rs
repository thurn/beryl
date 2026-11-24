//! A tooltip: words that explain the element the pointer rests on, drawn as the web
//! interface's `.tip` (`ui/src/design/components/tips.css`, `Tip` in
//! `ui/src/components/Controls.tsx`): a dark label under a thin metal rim, in small bold type
//! on one line. It may stand anywhere in the screen's tree; the flat layer shows it beside the
//! element it explains once the pointer has rested there (`layout::tip`).

use crate::element::{Anchor, Element, Presence, Timing, TipPlacement};
use crate::layout::TIP_PREFIX;
use kit_base::data::flat::Tooltip;
use kit_base::pose::Curve;
use kit_base::tokens::{color, shadow, size, type_style};
use kit_frame::frame::Role;

/// The tooltip saying `words` about the element named `of`, named `tooltip <of>`. It never
/// takes a press.
#[must_use]
pub fn tooltip(words: &str, of: &str, metrics: Tooltip) -> Element {
    build(words.to_owned(), of, metrics, None)
}

/// A tooltip that opens as soon as the pointer enters its target, on the given side.
#[must_use]
pub fn hover_tip(words: String, of: &str, metrics: Tooltip, placement: TipPlacement) -> Element {
    build(words, of, metrics, Some(placement))
}

fn build(words: String, of: &str, metrics: Tooltip, placement: Option<TipPlacement>) -> Element {
    let mut element = tip_label(words)
        .name(&format!("{TIP_PREFIX}{of}"))
        .enter(Presence::fade(Timing::new(
            metrics.fade_ms,
            0.0,
            Curve::EASE_OUT,
        )))
        .passes(true);
    element.anchor = Some(Anchor::Tip(placement));
    element
}

/// `words` drawn as a tooltip is, for a caller that places the label itself instead of leaving
/// it to wait for the pointer to rest (`layout::tip`), as a pile's count does. It takes the
/// words whole so as not to copy them, and leaves the element unnamed for the caller to name.
#[must_use]
pub fn tip_label(words: String) -> Element {
    let mut element = Element::new(Role::Text)
        .style(type_style::TIP)
        .background(color::TIP)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::TIP_RADIUS)
        .color(color::TEXT);
    element.text = Some(words);
    let (y, x) = (size::TIP_PADDING_Y, size::TIP_PADDING_X);
    element.style.padding = [y, x, y, x];
    element.style.box_shadow = shadow::TIP.to_vec();
    element
}
