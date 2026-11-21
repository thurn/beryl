//! Lists and menus (docs/beryl-3d.md, "Widgets"): where a menu and its submenu stand, and how
//! the keys and the pointer move the highlighted row. The highlighted row is the focused
//! element, so a row is drawn highlighted by its `FOCUSED` state and Return presses it as it
//! presses any focused element. The rules follow the web interface's `Menu.tsx` and
//! `menuKeys.ts` (`ui/src/table`): up and down move through a list's rows and go round from
//! the last to the first, the right arrow or Return opens a row's submenu on its first row, and
//! the left arrow closes it again.

use glam::Vec2;

use super::{Layer, Node};
use crate::element::{Action, Anchor, Display};
use crate::input::{Key, KeyCode, KeyPhase, Rect, Size};
use kit_frame::frame::{ElementId, Role};

/// What a key did to the list or menu that has the focus (`Layer::navigate`).
#[derive(Clone, Debug, PartialEq)]
pub enum Navigation {
    /// The key means nothing to a list, or no list has the focus.
    Ignored,
    /// It moved the highlighted row, or opened or closed a submenu.
    Moved,
    /// It pressed the highlighted row, which means this.
    Press(Action),
}

/// What a key or the pointer changes in a list or menu: the row to focus, the list to scroll
/// so that the row is seen, and what becomes of the open submenu.
struct Change {
    focus: ElementId,
    scroll: Option<ElementId>,
    submenu: Submenu,
}

/// What a `Change` does to the open submenu.
#[derive(Clone, Copy)]
enum Submenu {
    /// It stays as it is.
    Keep,
    /// The submenu of this row opens, and any other closes.
    Open(ElementId),
    /// Every submenu closes.
    Close,
}

/// An Escape press that asks the owner of the open menu to dismiss it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DismissMenu;

impl Layer {
    /// Takes `key` for the list or menu the focused element is in, or that has the focus
    /// itself. Down and up highlight the next or the previous row, going round, and the first
    /// or the last row when the list itself has the focus; the right arrow, or Return on a row
    /// with a submenu, opens the submenu on its first row; the left arrow in a submenu goes back
    /// to its row; and Return on any other row presses it. A disabled row is passed over.
    pub fn navigate(&mut self, key: &Key) -> Navigation {
        let Some(focused) = self.focused else {
            return Navigation::Ignored;
        };
        let Some(screen) = &self.screen else {
            return Navigation::Ignored;
        };
        let mut path = Vec::new();
        if !path_to(screen, focused, &mut path) {
            return Navigation::Ignored;
        }
        let Some(at) = path
            .iter()
            .rposition(|node| node.element.role == Role::List)
        else {
            return Navigation::Ignored;
        };
        let list = path[at];
        let row = (at + 1 < path.len()).then(|| path[path.len() - 1]);
        let in_submenu = matches!(list.element.anchor, Some(Anchor::Submenu));
        let enter = key.code == KeyCode::Enter && key.phase == KeyPhase::Down;
        // The right arrow, or Return, on a row with a submenu opens it.
        let opens = (key.code == KeyCode::ArrowRight || enter)
            .then_some(row)
            .flatten()
            .and_then(|row| submenu_of(row).map(|submenu| (row, submenu)));
        let change = if let Some((row, submenu)) = opens {
            let Some(first) = rows_of(submenu).first().map(|first| first.id) else {
                return Navigation::Ignored;
            };
            Change {
                focus: first,
                scroll: None,
                submenu: Submenu::Open(row.id),
            }
        } else {
            match key.code {
                KeyCode::ArrowDown | KeyCode::ArrowUp => {
                    let rows = rows_of(list);
                    let count = rows.len();
                    if count == 0 {
                        return Navigation::Ignored;
                    }
                    let down = key.code == KeyCode::ArrowDown;
                    let index = row.and_then(|row| rows.iter().position(|each| each.id == row.id));
                    let next = match (index, down) {
                        (Some(index), true) => (index + 1) % count,
                        (Some(index), false) => (index + count - 1) % count,
                        (None, true) => 0,
                        (None, false) => count - 1,
                    };
                    Change {
                        focus: rows[next].id,
                        scroll: Some(list.id),
                        // Moving among a menu's own rows closes the submenu of the row it
                        // leaves.
                        submenu: if in_submenu {
                            Submenu::Keep
                        } else {
                            Submenu::Close
                        },
                    }
                }
                KeyCode::ArrowLeft if in_submenu && at > 0 => Change {
                    focus: path[at - 1].id,
                    scroll: None,
                    submenu: Submenu::Close,
                },
                KeyCode::Enter if enter => {
                    return row
                        .filter(|row| row.element.enabled)
                        .and_then(|row| row.element.press.clone())
                        .map_or(Navigation::Ignored, Navigation::Press);
                }
                _ => return Navigation::Ignored,
            }
        };
        self.apply(&change);
        Navigation::Moved
    }

    /// Whether a context menu is shown on the screen (`Anchor::Menu`).
    #[must_use]
    pub fn menu_open(&self) -> bool {
        self.screen.as_ref().and_then(menu_in).is_some()
    }

    /// Whether a press at `point` falls outside the open context menu and its submenu, and so
    /// closes the menu, as a press on the web's `.table-menu-layer` around it does. `false`
    /// while no menu is open or the screen is not `open`.
    #[must_use]
    pub fn outside_menu(&self, point: Vec2, open: bool) -> bool {
        open && self.menu_open()
            && self
                .hit(point, open, |node, _| {
                    matches!(node.element.anchor, Some(Anchor::Menu(_) | Anchor::Submenu))
                        .then_some(())
                })
                .is_none()
    }

    /// The press Escape sends while a context menu is open: one that closes it.
    pub(super) fn menu_close_press(&self) -> Option<Action> {
        self.menu_open().then(|| Action::new(DismissMenu))
    }

    /// The pointer has come onto the menu row `id`, as the web's `onPointerEnter` on a row
    /// says: the row is highlighted, its submenu opens if it has one, and the submenu of
    /// another row of the same menu closes.
    pub(super) fn point_at_row(&mut self, id: ElementId) {
        let Some(screen) = &self.screen else {
            return;
        };
        let mut path = Vec::new();
        if !path_to(screen, id, &mut path) {
            return;
        }
        let row = path[path.len() - 1];
        if row.element.role != Role::MenuItem || !row.element.enabled {
            return;
        }
        let in_submenu = (path.iter().rev())
            .find(|node| node.element.role == Role::List)
            .is_some_and(|list| matches!(list.element.anchor, Some(Anchor::Submenu)));
        let submenu = if submenu_of(row).is_some() {
            Submenu::Open(row.id)
        } else if in_submenu {
            Submenu::Keep
        } else {
            Submenu::Close
        };
        let change = Change {
            focus: row.id,
            scroll: None,
            submenu,
        };
        self.apply(&change);
    }

    /// Keeps the menus right after the screen is shown again: the open submenu stays open if
    /// its row is still shown, and a context menu that does not hold the focus takes it on its
    /// first row, as the web's menu highlights its first row when it opens.
    pub(super) fn menus_shown(&mut self) {
        let row = self.submenu.filter(|&row| self.find(row).is_some());
        self.open_submenu(row);
        let first = self.screen.as_ref().and_then(menu_in).and_then(|menu| {
            let holds = self.focused.is_some_and(|id| menu.find(id).is_some());
            (!holds)
                .then(|| rows_of(menu).first().map(|row| row.id))
                .flatten()
        });
        if first.is_some() {
            self.focused = first;
        }
    }

    /// Makes `change`: focuses its row, scrolls its list to show the row, and opens or closes
    /// a submenu.
    fn apply(&mut self, change: &Change) {
        self.focused = Some(change.focus);
        if let Some(list) = change.scroll {
            self.scroll_into_view(list, change.focus);
        }
        match change.submenu {
            Submenu::Keep => {}
            Submenu::Open(row) => self.open_submenu(Some(row)),
            Submenu::Close => self.open_submenu(None),
        }
    }

    /// Opens the submenu of the menu row `row`, closing any other, or closes every submenu
    /// with `None`.
    fn open_submenu(&mut self, row: Option<ElementId>) {
        fn visit(node: &mut Node, row: Option<ElementId>) {
            let parent = node.id;
            for child in &mut node.children {
                if matches!(child.element.anchor, Some(Anchor::Submenu)) {
                    child.shown = row == Some(parent);
                }
                visit(child, row);
            }
        }
        self.submenu = row;
        if let Some(screen) = &mut self.screen {
            visit(screen, row);
        }
    }

    /// Scrolls the list `list` as little as shows the whole of its row `row`, as the web's
    /// `scrollIntoView({ block: "nearest" })` does.
    fn scroll_into_view(&mut self, list: ElementId, row: ElementId) {
        let (Some(list_node), Some(row_node)) = (self.find(list), self.find(row)) else {
            return;
        };
        let (frame, rect, scroll) = (list_node.rect, row_node.rect, list_node.scroll.y);
        let top = rect.y - frame.y;
        let bottom = top + rect.height;
        let to = if top < scroll {
            top
        } else if bottom > scroll + frame.height {
            bottom - frame.height
        } else {
            scroll
        };
        self.scroll(list, Vec2::new(0.0, to - scroll));
    }
}

/// Places every menu and submenu under `node`, whose tree's top left corner stands at `tree`
/// on a window of `window`'s size. `panel` is the box on the window of the menu `node` is in,
/// if any, beside which a submenu stands. Runs after each layout of the screen.
pub(super) fn place_menus(
    node: &mut Node,
    tree: Vec2,
    panel: Option<Rect>,
    window: Size,
    edge: f32,
) {
    // A submenu stands level with its row, which is `node`.
    let row_top = tree.y + node.rect.y;
    for child in &mut node.children {
        let size = Vec2::new(child.rect.width, child.rect.height);
        let corner = match child.element.anchor {
            Some(Anchor::Menu(at)) => Some(Vec2::new(
                open_at(at.x, size.x, window.width, edge),
                open_at(at.y, size.y, window.height, edge),
            )),
            Some(Anchor::Submenu) => panel.map(|panel| {
                let right = panel.x + panel.width;
                let x = if right + size.x + edge <= window.width {
                    right
                } else {
                    panel.x - size.x
                };
                Vec2::new(
                    fit(x, size.x, window.width, edge),
                    fit(row_top, size.y, window.height, edge),
                )
            }),
            Some(
                Anchor::Point(..) | Anchor::Object(..) | Anchor::Above(..) | Anchor::Below(..),
            ) => {
                if let Some(corner) = child.corner() {
                    place_menus(child, corner, panel, window, edge);
                }
                continue;
            }
            Some(Anchor::Tip) => continue,
            None => {
                place_menus(child, tree, panel, window, edge);
                continue;
            }
        };
        child.placed = corner;
        if let Some(corner) = corner {
            let rect = Rect {
                x: corner.x,
                y: corner.y,
                width: size.x,
                height: size.y,
            };
            place_menus(child, corner, Some(rect), window, edge);
        }
    }
}

/// Where a menu `size` long on an axis starts when it opens at `start` on a window `limit`
/// long, as `place` in `Menu.tsx` puts it: at `start`, or ending there if it would pass the
/// window's edge, then kept inside the window.
fn open_at(start: f32, size: f32, limit: f32, edge: f32) -> f32 {
    let start = if start + size + edge > limit {
        start - size
    } else {
        start
    };
    fit(start, size, limit, edge)
}

/// `start` moved as little as keeps a box `size` long the configured margin inside a window
/// `limit` long, or against its start where it is too long for that (`fit` in `Menu.tsx`).
pub(super) fn fit(start: f32, size: f32, limit: f32, edge: f32) -> f32 {
    edge.max(start.min(limit - size - edge))
}

/// The nodes from `node` down to the node `id`, pushed onto `path`; `false`, with `path` as it
/// was, when `id` is not under `node`.
fn path_to<'a>(node: &'a Node, id: ElementId, path: &mut Vec<&'a Node>) -> bool {
    path.push(node);
    if node.id == id || node.children.iter().any(|child| path_to(child, id, path)) {
        return true;
    }
    path.pop();
    false
}

/// The rows of the list `list` in tree order: the enabled elements under it that take the
/// focus or a press, without those of a list inside it, such as a submenu.
fn rows_of(list: &Node) -> Vec<&Node> {
    fn visit<'a>(node: &'a Node, rows: &mut Vec<&'a Node>) {
        for child in &node.children {
            let element = &child.element;
            if element.style.display == Display::None
                || element.anchor.is_some()
                || element.role == Role::List
            {
                continue;
            }
            if child.focusable() {
                if element.enabled {
                    rows.push(child);
                }
            } else {
                visit(child, rows);
            }
        }
    }
    let mut rows = Vec::new();
    visit(list, &mut rows);
    rows
}

/// The submenu of the menu row `row`, if it has one.
fn submenu_of(row: &Node) -> Option<&Node> {
    (row.children.iter()).find(|child| matches!(child.element.anchor, Some(Anchor::Submenu)))
}

/// The first context menu at or under `node`.
fn menu_in(node: &Node) -> Option<&Node> {
    if node.element.style.display == Display::None {
        return None;
    }
    if matches!(node.element.anchor, Some(Anchor::Menu(_))) {
        return Some(node);
    }
    node.children.iter().find_map(menu_in)
}
