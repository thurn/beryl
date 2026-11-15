//! Menus: a titled panel of rows in groups, and a context menu opened at a point.

use glam::Vec2;

use crate::element::{Anchor, Element, column, text};
use crate::frame::Role;

/// Which menu is open, so that a press on a row (`Press::Menu`) says whose row it was.
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
    /// The key that does the same, shown at the row's end.
    pub key: Option<String>,
    /// Whether the row shows a check mark.
    pub checked: bool,
    /// Whether a press on the row does anything now.
    pub enabled: bool,
}

/// The menu `id` titled `title`, with its rows in `groups`, each row a menu item named by its
/// label. Pressing a row is not wired until the widgets track builds menus.
#[must_use]
pub fn menu(id: MenuId, title: &str, groups: &[MenuGroup]) -> Element {
    let _ = id;
    column()
        .name(title)
        .children(groups.iter().flat_map(|group| {
            group.rows.iter().map(|row| {
                let mut item = text(&row.label).enabled(row.enabled);
                item.role = Role::MenuItem;
                item
            })
        }))
}

/// `menu` opened with its top left corner at `at`, a point of the window in logical points.
#[must_use]
pub fn context_menu(at: Vec2, menu: Element) -> Element {
    let mut menu = menu;
    menu.anchor = Some(Anchor::Point(at, Vec2::ZERO));
    menu
}
