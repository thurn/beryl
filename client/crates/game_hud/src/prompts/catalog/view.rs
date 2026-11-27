//! The prompt shape `Catalog`: one entry of a list, such as a card name, color, or creature type.
//!
//! The table is shaded and the entries stand in the catalog slot. Searchable catalogs (card names
//! and creature types) sit inside a framed panel with a `"Filter..."` search field above a
//! bounded scrolling list, matching Arena's card-name chooser (`xc8o0M3H124 8:47.0`). Short
//! catalogs (colors, Magical Hack's words) stay a plain scrolling column.

use beryl_protocol::prompt::{Click, PromptView, ShapeBody};
use glam::Vec2;

use super::{Catalog, FIELD, Message, State, is_searchable, matching};
use crate::hud::HudIn;
use crate::prompts::shade;
use kit_base::data::profiles::SlotName;
use kit_base::tokens::{color, size, space, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Anchor, Element, Justify, column, text};
use kit_ui::widgets::{field, list};

/// What the flat layer shows for `prompt`: the shade over the table, then the prompt bar, then
/// the catalog over the shade.
pub(super) fn view(state: &State, prompt: &PromptView, input: &HudIn, bar: Element) -> Element {
    let mut bar = bar;
    if let ShapeBody::Catalog { entries, chosen } = &prompt.body {
        bar.children.insert(0, shade(input));
        let catalog = if is_searchable(prompt) {
            searchable_panel(state, entries, *chosen, input)
        } else {
            entries_column(entries, *chosen, input)
        };
        bar.children.push(catalog);
    }
    bar
}

/// The catalog's available rectangle: the profile's `prompt.catalog` slot on phones (clamped
/// above `prompt.stacked` so the prompt bar never overlaps the list), or the tray space on
/// desktop.
fn catalog_space(input: &HudIn) -> kit_base::geometry::Rect {
    let mut space = input.profile.slot(SlotName::PromptCatalog).map_or_else(
        || input.profile.tray().on_window(input.window.size).0,
        |slot| slot.rect,
    );
    if let Some(stacked) = input.profile.slot(SlotName::PromptStacked) {
        let limit = stacked.rect.y - space::S;
        if space.y + space.height > limit {
            space.height = (limit - space.y).max(0.0);
        }
    }
    space
}

/// Framed search panel for card names and creature types: a `"Filter..."` text field above a
/// scrolling list of matching entries, bounded inside `catalog_space` so it never runs off screen
/// and keeping a steady top edge as the list filters (matching Arena `xc8o0M3H124 8:47.0`).
fn searchable_panel(
    state: &State,
    entries: &[String],
    chosen: Option<u16>,
    input: &HudIn,
) -> Element {
    let space = catalog_space(input);
    let tight = space.height < size::CATALOG_TIGHT_HEIGHT;
    let target = input.profile.data().checks.target_points;
    let entry_height = if tight {
        target.max(size::CATALOG_TIGHT_ENTRY_HEIGHT)
    } else {
        size::ENTRY_HEIGHT
    };
    let panel_padding = if tight { 2.0 } else { size::MENU_PADDING };
    let panel_gap = if tight { 2.0 } else { space::M };
    let row_gap = if tight { space::S } else { size::ENTRY_GAP };

    let mut field_state = input.fields.get(FIELD);
    FIELD.clone_into(&mut field_state.placeholder);
    let query = field_state.text.clone();

    let matched = matching(&query, entries);
    let highlight = if matched.is_empty() {
        0
    } else {
        state.highlight.min(matched.len() - 1)
    };

    let rows: Vec<Element> = if matched.is_empty() {
        vec![
            text("No matching names")
                .style(type_style::BODY)
                .color(color::TEXT_DIM),
        ]
    } else {
        matched
            .iter()
            .enumerate()
            .map(|(row, &entry_index)| {
                let index = u16::try_from(entry_index).unwrap_or(u16::MAX);
                let is_chosen = chosen == Some(index);
                let is_highlighted = row == highlight;
                searchable_entry(
                    &entries[entry_index],
                    index,
                    row,
                    entry_height,
                    is_chosen,
                    is_highlighted,
                )
                .shrink(0.0)
            })
            .collect()
    };

    let field_height = (type_style::FIELD.size * type_style::FIELD.line_height
        + 2.0 * size::FIELD_PADDING_Y
        + 2.0 * size::FIELD_BORDER)
        .max(target);
    let chrome = 2.0 * panel_padding + 2.0 * size::MENU_BORDER + field_height + panel_gap;
    let list_max = (space.height - chrome).max(0.0);
    let panel_width = (space.width - 2.0 * space::M)
        .min(size::CATALOG_SEARCH_MAX_WIDTH)
        .max(size::ENTRY_MIN_WIDTH.min(space.width));

    let mut search = field(FIELD, &field_state).min_height(target).shrink(0.0);
    if tight {
        search.style.padding = [
            size::CATALOG_TIGHT_FIELD_PADDING_Y,
            size::FIELD_PADDING_X,
            size::CATALOG_TIGHT_FIELD_PADDING_Y,
            size::FIELD_PADDING_X,
        ];
    }
    let entries_list = list("entries", rows)
        .gap(row_gap)
        .align_items(Align::Stretch)
        .max_height(list_max);

    let panel = column()
        .name("search panel")
        .width(panel_width)
        .height(space.height)
        .gap(panel_gap)
        .padding(panel_padding)
        .background(color::SURFACE)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::MENU_RADIUS)
        .align_items(Align::Stretch)
        .child(search)
        .child(entries_list);

    let mut placed = column()
        .name("catalog")
        .width(space.width)
        .height(space.height)
        .justify_content(Justify::Center)
        .align_items(Align::Center)
        .child(panel);
    placed.anchor = Some(Anchor::Point(Vec2::new(space.x, space.y), Vec2::ZERO));
    placed
}

/// Plain scrolling column for short catalogs (colors, Magical Hack words).
fn entries_column(entries: &[String], chosen: Option<u16>, input: &HudIn) -> Element {
    let space = catalog_space(input);
    let rows = (entries.iter().enumerate()).map(|(index, words)| {
        let index = u16::try_from(index).unwrap_or(u16::MAX);
        entry(words, index, chosen == Some(index)).shrink(0.0)
    });
    let entries = list("entries", rows)
        .gap(size::ENTRY_GAP)
        .align_items(Align::Stretch)
        .max_height(space.height);
    let mut placed = column()
        .name("catalog")
        .width(space.width)
        .height(space.height)
        .justify_content(Justify::Center)
        .align_items(Align::Center)
        .child(entries);
    placed.anchor = Some(Anchor::Point(Vec2::new(space.x, space.y), Vec2::ZERO));
    placed
}

/// One entry in a searchable catalog: gold when `chosen`, highlighted as the active row when
/// `highlighted`, and sending `Message::Pick` with its session index and visible row.
fn searchable_entry(
    words: &str,
    index: u16,
    row: usize,
    height: f32,
    chosen: bool,
    highlighted: bool,
) -> Element {
    let mut element = Element::new(Role::Button).name(words);
    element.text = Some(words.to_owned());
    element.style.text = type_style::ENTRY;
    element.style.text_align = kit_ui::element::TextAlign::Center;
    element.style.padding = [0.0, size::ENTRY_PADDING, 0.0, size::ENTRY_PADDING];
    let (face, rim) = if chosen {
        (color::ENTRY_CHOSEN, color::GOLD)
    } else if highlighted {
        (color::MENU_ACTIVE, color::GOLD)
    } else {
        (color::ENTRY, color::ENTRY_RIM)
    };
    element
        .height(height)
        .radius(height / 2.0)
        .background(face)
        .border((rim, size::TRAY_PILE_BORDER))
        .color(color::TEXT)
        .selected(chosen)
        .on_press(Message::Pick { entry: index, row })
}

/// One entry in a plain catalog: a pill showing `words` that sends its `index`, lit gold while
/// it is the one `chosen`.
fn entry(words: &str, index: u16, chosen: bool) -> Element {
    let mut element = Element::new(Role::Button).name(words);
    element.text = Some(words.to_owned());
    element.style.text = type_style::ENTRY;
    element.style.text_align = kit_ui::element::TextAlign::Center;
    element.style.padding = [0.0, size::ENTRY_PADDING, 0.0, size::ENTRY_PADDING];
    let (face, rim) = if chosen {
        (color::ENTRY_CHOSEN, color::GOLD)
    } else {
        (color::ENTRY, color::ENTRY_RIM)
    };
    element
        .min_width(size::ENTRY_MIN_WIDTH)
        .height(size::ENTRY_HEIGHT)
        .radius(size::ENTRY_HEIGHT / 2.0)
        .background(face)
        .border((rim, size::TRAY_PILE_BORDER))
        .color(color::TEXT)
        .selected(chosen)
        .on_press(Catalog::message(Click::Entry { index }))
}
