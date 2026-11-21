//! Menus, with the look and placement of the web interface's `Menu` (`ui/src/table/Menu.tsx`
//! and `menu.css`): a dark panel under a thin metal rim holding a title and then rows in
//! groups, each group under a line. A row shows a check mark if it is checked, its label, and
//! at its end the key that does the same, or a chevron if it opens a submenu. The highlighted
//! row is the focused one, drawn on a gold tint (`layout::write`); the keys and the pointer
//! move it (`layout::menu`). A context menu opens at a point and stays inside the window, and
//! a submenu opens beside its row.

use glam::Vec2;

use crate::element::{Align, Anchor, Element, column, row, text};
use kit_base::tokens::{color, shadow, size, type_style};
use kit_frame::frame::Role;

/// The selected row of an open menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuChoice(pub MenuId, pub u16);

/// Which menu is open, so that a press on a row (`MenuChoice`) says whose row it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MenuId(pub u16);

/// The rows of a menu between two dividers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuGroup {
    pub rows: Vec<MenuRow>,
}

/// One row of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuRow {
    pub label: String,
    /// The words of the key that does the same, shown at the row's end, as
    /// `element::describe` writes a binding for the host's platform.
    pub key: Option<String>,
    /// Whether the row shows a check mark.
    pub checked: bool,
    /// Whether a press on the row does anything now.
    pub enabled: bool,
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
            submenu: Vec::new(),
        }
    }
}

/// The menu `id` titled `title`, with `whose` under the title (whose the card or pile is, or
/// nothing), and its rows in `groups`: a list named by its title whose rows are menu items
/// named by their labels. A press on a row sends `MenuChoice(id, n)`, where `n` counts the
/// menu's rows from 0 over all its groups, and then the rows of its submenus, in order, after
/// the last of them.
#[must_use]
pub fn menu(id: MenuId, title: &str, whose: &str, groups: &[MenuGroup]) -> Element {
    build(id, (title, whose), groups, false, None)
}

/// The press of the Back row of a sheet menu showing a submenu's rows (`sheet_menu`), which
/// no row of a menu has.
pub const BACK: u16 = u16::MAX;

/// `menu` as the phone's bottom sheet shows it (`Menu.tsx`'s pages): a row with a submenu
/// presses `MenuChoice(id, n)` like any row, and opens no list beside the menu. With `page`
/// set to the number of such a row, the sheet instead holds a Back row, pressing
/// `MenuChoice(id, BACK)`, and the rows of that row's submenu, numbered as `menu` numbers
/// them.
#[must_use]
pub fn sheet_menu(
    id: MenuId,
    title: &str,
    whose: &str,
    groups: &[MenuGroup],
    page: Option<u16>,
) -> Element {
    build(id, (title, whose), groups, true, page)
}

/// The panel of `menu` and `sheet_menu`, the latter if `sheet` is set, under the title and
/// the dim line that says whose the target is (`Menu.tsx`'s `table-menu-title`).
fn build(
    id: MenuId,
    (title, whose): (&str, &str),
    groups: &[MenuGroup],
    sheet: bool,
    page: Option<u16>,
) -> Element {
    let rows = groups.iter().map(|group| group.rows.len()).sum::<usize>();
    let mut next = 0;
    let mut next_nested = u16::try_from(rows).unwrap_or(u16::MAX);
    let mut heading = text(title).style(type_style::MENU_TITLE).color(color::TEXT);
    heading.style.padding = [
        size::MENU_TITLE_PADDING_TOP,
        size::MENU_TITLE_PADDING_SIDE,
        size::MENU_TITLE_PADDING_BOTTOM,
        size::MENU_TITLE_PADDING_SIDE,
    ];
    let mut panel = panel(title).child(heading);
    if sheet {
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
    if let Some(page) = page.filter(|_| sheet) {
        // The rows of the submenu of row `page` take their numbers after those of the
        // submenus before it.
        for (folder, row) in groups.iter().flat_map(|group| &group.rows).enumerate() {
            if folder == usize::from(page) {
                let mut deeper = u16::MAX;
                let back = menu_row(id, &MenuRow::new("Back"), &mut 0, &mut deeper, true)
                    .on_press(MenuChoice(id, BACK));
                panel = panel.child(divider(0, "page")).child(back);
                for child in &row.submenu {
                    panel = panel.child(menu_row(id, child, &mut next_nested, &mut deeper, true));
                }
                return panel;
            }
            next_nested += u16::try_from(row.submenu.len()).unwrap_or(0);
        }
    }
    for (index, group) in groups.iter().enumerate() {
        panel = panel.child(divider(index, &format!("group {index}")));
        for row in &group.rows {
            panel = panel.child(menu_row(id, row, &mut next, &mut next_nested, sheet));
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
    menu.style.enter = Some(&super::screen::SCREEN_FADE);
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

/// The menu item for `row`. A row without a submenu, or any row if `flat`, presses `MenuChoice(id, *next)`, taking
/// that number; a row with one opens it, whose rows take numbers from `*nested` on.
fn menu_row(
    id: MenuId,
    row_view: &MenuRow,
    next: &mut u16,
    nested: &mut u16,
    flat: bool,
) -> Element {
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
    if row_view.submenu.is_empty() || flat {
        item = item.on_press(MenuChoice(id, *next));
    } else {
        // A submenu has no submenus of its own (`Menu.tsx` opens one level), so the numbers a
        // row of it would give a deeper one are not kept.
        let mut deeper = u16::MAX;
        let mut submenu = panel(&row_view.label);
        for child in &row_view.submenu {
            submenu = submenu.child(menu_row(id, child, nested, &mut deeper, false));
        }
        submenu.anchor = Some(Anchor::Submenu);
        item = item.child(submenu);
    }
    *next += 1;
    item
}
