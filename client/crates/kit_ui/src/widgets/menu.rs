//! Menus, with the look and placement of the web interface's `Menu` (`ui/src/table/Menu.tsx`
//! and `menu.css`): a dark panel under a thin metal rim holding a title and then rows in
//! groups, each group under a line. A row shows a check mark if it is checked, its label, and
//! at its end the key that does the same, or a chevron if it opens a submenu. The highlighted
//! row is the focused one, drawn on a gold tint (`layout::write`); the keys and the pointer
//! move it (`layout::menu`). A context menu opens at a point and stays inside the window, and
//! a submenu opens beside its row.

use glam::Vec2;

use crate::element::{Action, Align, Anchor, Element, column, row, text};
use kit_base::tokens::{color, shadow, size, type_style};
use kit_frame::frame::Role;

/// The folder whose contents are shown on the phone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuPage {
    group: usize,
    row: usize,
}

/// The rows of a menu between two dividers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuGroup {
    pub rows: Vec<MenuRow>,
}

/// One row of a menu.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuRow {
    pub label: String,
    /// The words of the key that does the same, shown at the row's end, as
    /// `element::describe` writes a binding for the host's platform.
    pub key: Option<String>,
    /// Whether the row shows a check mark.
    pub checked: bool,
    /// Whether a press on the row does anything now.
    pub enabled: bool,
    /// The owning feature’s message for choosing this item.
    pub action: Option<Action>,
    /// The rows of the submenu it opens, if any. A row with a submenu sends no press of its
    /// own: it opens the submenu.
    pub submenu: Vec<MenuRow>,
}

impl MenuRow {
    /// An enabled row showing `label`, with no key, check mark or submenu.
    #[must_use]
    pub fn new(label: &str) -> Self {
        Self {
            label: label.to_owned(),
            key: None,
            checked: false,
            enabled: true,
            action: None,
            submenu: Vec::new(),
        }
    }
}

/// A list of grouped items whose presses send the actions supplied with each row.
#[must_use]
pub fn menu(title: &str, whose: &str, groups: &[MenuGroup]) -> Element {
    build((title, whose), groups, None)
}

/// A phone menu. A folder opens a page in the same sheet, and Back returns to the groups.
#[must_use]
pub fn sheet_menu(
    title: &str,
    whose: &str,
    groups: &[MenuGroup],
    page: Option<MenuPage>,
    navigate: fn(Option<MenuPage>) -> Action,
) -> Element {
    build((title, whose), groups, Some((page, navigate)))
}

type Sheet = (Option<MenuPage>, fn(Option<MenuPage>) -> Action);

fn build((title, whose): (&str, &str), groups: &[MenuGroup], sheet: Option<Sheet>) -> Element {
    let mut heading = text(title).style(type_style::MENU_TITLE).color(color::TEXT);
    heading.style.padding = [
        size::MENU_TITLE_PADDING_TOP,
        size::MENU_TITLE_PADDING_SIDE,
        size::MENU_TITLE_PADDING_BOTTOM,
        size::MENU_TITLE_PADDING_SIDE,
    ];
    let mut panel = panel(title).child(heading);
    if sheet.is_some() {
        panel.style.box_shadow = shadow::SHEET.to_vec();
    }
    if !whose.is_empty() {
        let mut line = text(whose)
            .style(type_style::MENU_HINT)
            .color(color::TEXT_DIM);
        line.style.padding = [
            0.0,
            size::MENU_TITLE_PADDING_SIDE,
            size::MENU_TITLE_PADDING_BOTTOM,
            size::MENU_TITLE_PADDING_SIDE,
        ];
        panel = panel.child(line);
    }
    if let Some((Some(page), navigate)) = sheet
        && let Some(folder) = groups
            .get(page.group)
            .and_then(|group| group.rows.get(page.row))
    {
        let mut back = MenuRow::new("Back");
        back.action = Some(navigate(None));
        return panel
            .child(divider(0, "page"))
            .child(menu_row(&back, None))
            .children(folder.submenu.iter().map(|row| menu_row(row, None)));
    }
    for (group, items) in groups.iter().enumerate() {
        panel = panel.child(divider(group, &format!("group {group}")));
        for (row, item) in items.rows.iter().enumerate() {
            let page = sheet
                .filter(|_| !item.submenu.is_empty())
                .map(|(_, navigate)| navigate(Some(MenuPage { group, row })));
            panel = panel.child(menu_row(item, page));
        }
    }
    panel
}

/// The line above a group of rows, the first group's included.
fn divider(index: usize, name: &str) -> Element {
    let gap = size::MENU_GROUP_GAP;
    row()
        .name(name)
        .height(size::MENU_BORDER)
        .background(color::METAL)
        .margin([if index == 0 { 0.0 } else { gap }, 0.0, gap, 0.0])
}

/// `menu` opened at `at`, a point of the window in logical points, as a context menu opens
/// where the pointer was (`Anchor::Menu`).
#[must_use]
pub fn context_menu(at: Vec2, menu: Element) -> Element {
    let mut menu = menu;
    menu.anchor = Some(Anchor::Menu(at));
    menu
}

/// A menu's panel, a list named `name`, as wide as its rows within the menu's least and most
/// widths.
fn panel(name: &str) -> Element {
    let mut panel = column()
        .name(name)
        .min_width(size::MENU_MIN_WIDTH)
        .max_width(size::MENU_MAX_WIDTH)
        .padding(size::MENU_PADDING)
        .background(color::SURFACE)
        .border((color::METAL, size::MENU_BORDER))
        .radius(size::MENU_RADIUS);
    panel.style.box_shadow = shadow::LIFTED.to_vec();
    panel.role = Role::List;
    panel
}

/// A folder shows its desktop submenu or sends the phone’s page action.
fn menu_row(row_view: &MenuRow, page: Option<Action>) -> Element {
    let enabled = row_view.enabled;
    let hint = if row_view.submenu.is_empty() {
        row_view.key.clone()
    } else {
        Some("›".to_owned())
    };
    let check = row().name("check").width(size::MENU_CHECK).children(
        row_view
            .checked
            .then(|| super::check_mark(size::MENU_CHECK * 0.5)),
    );
    let mut item = Element::new(Role::MenuItem)
        .name(&row_view.label)
        .enabled(enabled)
        .min_height(size::MENU_ROW_HEIGHT)
        .align_items(Align::Center)
        .gap(size::MENU_ROW_GAP)
        .child(check)
        .child(
            text(&row_view.label)
                .style(type_style::MENU)
                .enabled(enabled)
                .grow(1.0),
        )
        .children(hint.map(|hint| {
            text(&hint)
                .name("key")
                .style(type_style::MENU_HINT)
                .color(color::TEXT_DIM)
                .enabled(enabled)
        }));
    item.style.padding = [
        size::MENU_ROW_PADDING_Y,
        size::MENU_ROW_PADDING_END,
        size::MENU_ROW_PADDING_Y,
        size::MENU_ROW_PADDING_START,
    ];
    // A press on a row highlights it, and a row with a submenu, which has no press, can still
    // be highlighted by the keys.
    item.focus = true;
    if row_view.submenu.is_empty() {
        item.press.clone_from(&row_view.action);
    } else if let Some(page) = page {
        item.press = Some(page);
    } else {
        let mut submenu =
            panel(&row_view.label).children(row_view.submenu.iter().map(|row| menu_row(row, None)));
        submenu.anchor = Some(Anchor::Submenu);
        item = item.child(submenu);
    }
    item
}
