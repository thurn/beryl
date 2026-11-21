//! The manual table's pile viewer controls and card grouping.

use beryl_protocol::protocol::ZoneName;

use super::TablePress;
use super::viewers::{PileViewer, SEARCH, Sort, sections};
use crate::element::{Align, Element, Overflow, column, row, text};
use crate::hud::GamePress;
use crate::hud::{HudIn, piles::pile_cards};
use crate::inspect::Viewed;
use crate::inspect::viewer::{
    self, cards, count_words, grid, object_box, pile_title, viewer_dialog,
};
use crate::widgets::{Fields, button, field, primary};
use kit_base::tokens::{color, size, space, type_style};

/// The viewer the manual table shows, if one is open, as the web's `TablePileViewer` shows it:
/// a pile's viewer, or a library's while the table lets the viewer look at it
/// (`TableState::viewer`), has a row of controls over its cards: the search field, "Group by
/// type" and the sort, which lists the cards in the pile's order, by name or by mana value
/// (`manual::viewers::sections`). Closing a search of a whole library first asks whether to
/// shuffle it: Shuffle, the highlighted choice, and Don't shuffle take the Close button's
/// place. The Revealed Cards pile's viewer is the game screen's.
#[must_use]
pub(crate) fn table_viewer(input: &HudIn, fields: &Fields, state: &PileViewer) -> Option<Element> {
    let Viewed::Pile(side, zone) = input.viewing? else {
        return viewer::viewer(input);
    };
    let view = input.view?;
    let cards = match &state.look {
        Some(look) if zone == ZoneName::Library => look.cards(view),
        _ => pile_cards(view, side, zone),
    };
    let query = fields.get(SEARCH).text.trim().to_lowercase();
    let sections = sections(view, input.faces, &cards, &query, state.sort, state.grouped);
    let mut boxes: Vec<(Option<String>, Vec<Element>)> = (sections.into_iter())
        .map(|section| {
            let boxes = section.cards.iter().map(|&slot| object_box(view, slot));
            (section.label, boxes.collect())
        })
        .collect();
    let body = if state.grouped {
        sections_grid(boxes)
    } else {
        grid(boxes.pop().map(|(_, boxes)| boxes).unwrap_or_default())
    };
    let top_first = state.sort == Sort::Pile && !state.grouped;
    let count = count_words(cards.len(), zone == ZoneName::Library, top_first);
    let actions = if state.confirming && state.can_shuffle(view) {
        vec![
            primary("Shuffle").on_press(TablePress::ViewerFinish(true)),
            button("Don't shuffle").on_press(TablePress::ViewerFinish(false)),
        ]
    } else {
        vec![
            button("Close")
                .name("close")
                .on_press(GamePress::DismissViewer),
        ]
    };
    let content = column()
        .name("browser")
        .align_items(Align::Center)
        .gap(space::M)
        .child(controls(fields, state))
        .child(body);
    Some(viewer_dialog(
        input.window.size,
        (pile_title(side, zone), &count),
        cards.len(),
        size::OVERLAY_MAIN + 2.0 * space::L,
        None,
        content,
        actions,
    ))
}

/// The row of the table viewer's controls: the search field, "Group by type", checked while
/// the viewer groups its cards, and the sort, which shows the order the viewer lists its cards
/// in and chooses the next when pressed.
fn controls(fields: &Fields, state: &PileViewer) -> Element {
    let mut search = fields.get(SEARCH);
    "Search".clone_into(&mut search.placeholder);
    let group = button("Group by type")
        .selected(state.grouped)
        .on_press(TablePress::ViewerGroup);
    let group = if state.grouped {
        group.color(color::EMBER)
    } else {
        group
    };
    let sort = button(state.sort.label())
        .name("Sort")
        .on_press(TablePress::ViewerSort(state.sort.next()));
    row()
        .name("controls")
        .width(size::OVERLAY_MAIN)
        .gap(space::S)
        .align_items(Align::Center)
        .child(field(SEARCH, &search).grow(1.0))
        .child(group)
        .child(sort)
}

/// The grids of `sections`, each under its label, in one column that scrolls past the grid's
/// height, or "Nothing here." for no card.
fn sections_grid(sections: Vec<(Option<String>, Vec<Element>)>) -> Element {
    if sections.is_empty() {
        return text("Nothing here.").color(color::TEXT_DIM);
    }
    let sections = sections.into_iter().map(|(label, boxes)| {
        let label = label.unwrap_or_default();
        column()
            .name(&label)
            .gap(space::S)
            .child(text(&label).style(type_style::OVERLAY_INSTRUCTION))
            .child(cards(boxes))
    });
    column()
        .name("cards")
        .gap(space::M)
        .max_height(size::VIEWER_HEIGHT)
        .overflow(Overflow::Scroll)
        .scroll_fade(size::VIEWER_FADE)
        .children(sections)
}
