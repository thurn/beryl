//! The desktop log column and the phone log sheet.

use glam::Vec2;

use super::{LIST, LogEntry, LogKind, Message};
use game_table::table::PhoneBands;
use kit_base::data::space::hud::{LOG_BUTTON_TOP, LOG_LEFT, LOG_TOP, phone as data};
use kit_base::tokens::{color, size, space, type_style};
use kit_ui::element::{Align, Anchor, Element, Justify, column, holder, row as flex_row, text};
use kit_ui::widgets::{icon_button, icons, list};

pub(super) fn desktop(open: bool, entries: &[LogEntry]) -> Element {
    let name = if open { "Hide log" } else { "Show log" };
    let mut button = icon_button(icons::LOG, name)
        .selected(open)
        .on_press(Message::Toggle);
    button.anchor = Some(Anchor::Point(
        Vec2::new(LOG_LEFT, LOG_BUTTON_TOP),
        Vec2::ZERO,
    ));
    let list = (open && !entries.is_empty()).then(|| {
        // On the desktop the oldest lines fade out under the list's top edge, as
        // `.game-log-list`'s mask fades them; the phone's sheet has no mask.
        let mut panel = (panel(entries)
            .width(size::LOG_WIDTH)
            .max_height(size::LOG_FOLDED_HEIGHT))
        .mask(kit_base::tokens::gradient::LOG_MASK)
        .enter(&kit_ui::widgets::screen::SCREEN_INSTANT);
        panel.anchor = Some(Anchor::Point(Vec2::new(LOG_LEFT, LOG_TOP), Vec2::ZERO));
        panel
    });
    holder().name("log").child(button).children(list)
}

/// The log's list of `entries`, oldest first, on its panel, with no size or place of its own:
/// the column at the table's left sets them on the desktop, and the log's sheet on the phone
/// (`hud::phone`).
fn panel(entries: &[LogEntry]) -> Element {
    let last = entries.len().saturating_sub(1);
    let rows = (entries.iter().enumerate()).map(|(at, entry)| {
        // A line said before, such as a second trigger of the same ability, is named with its
        // count, so that no two rows share a name.
        let before = (entries[..at].iter())
            .filter(|earlier| earlier.line.text == entry.line.text)
            .count();
        row(entry, before, (at == last, at == 0))
    });
    let mut panel = list(LIST, rows)
        .radius(size::LOG_RADIUS)
        .background(color::LOG);
    let (x, y) = (size::LOG_PADDING_X, size::LOG_PADDING_Y);
    panel.style.padding = [y, x, y, x];
    panel
}

/// One entry's row: a turn's heading in gold capitals, a countered spell in orange, anything
/// else in the dim text color, the newest in the text color. A row whose words `before` earlier
/// rows say too is named with its count, as "Blood Artist triggered (2)".
fn row(entry: &LogEntry, before: usize, (last, first): (bool, bool)) -> Element {
    let line = &entry.line;
    let (words, style, tint) = match line.kind {
        LogKind::Turn => (
            line.text.to_uppercase(),
            type_style::LOG_TURN,
            color::LOG_TURN,
        ),
        LogKind::Counter => (line.text.clone(), type_style::LOG, color::LOG_COUNTER),
        LogKind::Action if last => (line.text.clone(), type_style::LOG, color::LOG_LAST),
        LogKind::Action => (line.text.clone(), type_style::LOG, color::LOG_TEXT),
    };
    let name = if before == 0 {
        line.text.clone()
    } else {
        format!("{} ({})", line.text, before + 1)
    };
    let row = text(&words).style(style).color(tint).name(&name);
    if line.kind == LogKind::Turn && !first {
        row.margin([size::LOG_TURN_GAP, 0.0, 0.0, 0.0])
    } else {
        row
    }
}

/// The log as a sheet over the table while it is open (`LogSheet` in
/// `ui/src/stage/PhoneChrome.tsx`): the column's width, between the opponent's strip and the hand,
/// with its heading and a button that closes it over the list, which scrolls to its foot as the
/// desktop's does.
pub(super) fn phone(entries: &[LogEntry], bands: &PhoneBands) -> Element {
    // Below the opponent's strip, so that the menu button stays clear of it.
    let margin = data::SHEET_MARGIN;
    let top = bands.opponent_strip.bottom() + margin;
    let height = (bands.hand.top - margin - top).max(0.0);
    let heading = flex_row()
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(text("Log").style(type_style::TITLE))
        .child(icon_button(icons::CLOSE, "Close log").on_press(Message::Toggle));
    let list = panel(entries)
        .grow(1.0)
        .shrink(1.0)
        .min_height(size::ICON_BUTTON);
    let mut sheet = column()
        .name("log sheet")
        .width(bands.right - bands.left)
        .max_height(height)
        .padding(space::M)
        .gap(space::S)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .child(heading)
        .child(list);
    sheet.anchor = Some(Anchor::Point(Vec2::new(bands.left, top), Vec2::ZERO));
    sheet
}
