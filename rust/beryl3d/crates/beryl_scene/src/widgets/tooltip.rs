//! A tooltip: words that explain the element the pointer rests on, drawn as the web
//! interface's `.tip` (`ui/src/design/components/tips.css`, `Tip` in
//! `ui/src/components/Controls.tsx`): a dark label under a thin metal rim, in small bold type
//! on one line. It may stand anywhere in the screen's tree; the flat layer shows it beside the
//! element it explains once the pointer has rested there (`layout::tip`).

use crate::element::{Anchor, Element, text};
use crate::layout::TIP_PREFIX;
use crate::tokens::{color, size, type_style};

/// The tooltip saying `words` about the element named `of`, named `tooltip <of>`. It never
/// takes a press.
#[must_use]
pub fn tooltip(words: &str, of: &str) -> Element {
    let mut element = text(words)
        .name(&format!("{TIP_PREFIX}{of}"))
        .style(type_style::TIP)
        .background(color::TIP)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::TIP_RADIUS)
        .color(color::TEXT);
    let (y, x) = (size::TIP_PADDING_Y, size::TIP_PADDING_X);
    element.style.padding = [y, x, y, x];
    element.anchor = Some(Anchor::Tip);
    element
}
