//! Keyboard focus (docs/beryl-3d.md, "Elements"): one element of the screen at a time takes
//! the keys. A press on an element with `focus: true` or a focus message gives it the focus,
//! Tab and Shift+Tab move it among the focusable elements in tree order, as a browser moves it
//! among buttons and fields, and Return presses the focused element.

use glam::Vec2;

use super::{Layer, Node};
use crate::element::{Action, Display};
use crate::frame::ElementId;
use crate::input::Key;

impl Layer {
    /// The focused element, if any.
    #[must_use]
    pub fn focused(&self) -> Option<ElementId> {
        self.focused
    }

    /// Gives the focus to the element `id`, or takes it away with `None`.
    pub fn focus(&mut self, id: Option<ElementId>) {
        self.focused = id;
        self.follow_focus();
    }

    /// Gives the focus to the text field named `name`, if the screen has one, as an
    /// `autoFocus` field takes it when it appears.
    pub fn focus_field(&mut self, name: &str) {
        fn search(node: &Node, name: &str) -> Option<ElementId> {
            if node.element.field.is_some() && node.element.name == name {
                return Some(node.id);
            }
            node.children.iter().find_map(|child| search(child, name))
        }
        if let Some(id) = self.screen.as_ref().and_then(|screen| search(screen, name)) {
            self.focus(Some(id));
        }
    }

    /// The innermost element at `point` that takes the focus when pressed, leaving out what
    /// waits for the table (`Element::waits`) unless the layer is `open`.
    #[must_use]
    pub fn focusable_at(&self, point: Vec2, open: bool) -> Option<ElementId> {
        self.hit(point, open, |node, _| node.element.focus.then_some(node.id))
    }

    /// Moves the focus to the next focusable element in tree order, or the one before if not
    /// `forward`, going round from the last to the first. With nothing focused, it goes to the
    /// first, or the last. Allocates nothing.
    pub fn move_focus(&mut self, forward: bool) {
        /// Calls `visit` on every focusable node under `node`, in tree order.
        fn each(node: &Node, visit: &mut impl FnMut(ElementId)) {
            if node.element.style.display == Display::None {
                return;
            }
            if node.focusable() {
                visit(node.id);
            }
            for child in &node.children {
                each(child, visit);
            }
        }
        let Some(screen) = &self.screen else {
            return;
        };
        let current = self.focused;
        let (mut first, mut last, mut before, mut after) = (None, None, None, None);
        let mut passed = false;
        each(screen, &mut |id| {
            first = first.or(Some(id));
            last = Some(id);
            if Some(id) == current {
                passed = true;
            } else if passed {
                after = after.or(Some(id));
            } else {
                before = Some(id);
            }
        });
        let next = match (forward, passed) {
            (true, true) => after.or(first),
            (false, true) => before.or(last),
            (true, false) => first,
            (false, false) => last,
        };
        self.focused = next;
        self.follow_focus();
    }

    /// What pressing the focused element means, if it is enabled and has a press.
    #[must_use]
    pub fn focused_press(&self) -> Option<Action> {
        let id = self.focused?;
        let node = self.find(id)?;
        (node.element.enabled)
            .then(|| node.element.press.clone())
            .flatten()
    }

    /// The press Escape sends: one that closes the open context menu, or else the press of the
    /// last enabled element named `close` on the screen, the close button of the sheet drawn
    /// over the rest.
    #[must_use]
    pub fn close_press(&self) -> Option<Action> {
        fn search(node: &Node) -> Option<Action> {
            if node.element.style.display == Display::None {
                return None;
            }
            (node.children.iter().rev()).find_map(search).or_else(|| {
                let element = &node.element;
                (element.name == "close" && element.enabled)
                    .then(|| element.press.clone())
                    .flatten()
            })
        }
        (self.menu_close_press()).or_else(|| self.screen.as_ref().and_then(search))
    }

    /// The press the screen's top element gives `key` as a shortcut, on the Mac if `mac`.
    #[must_use]
    pub fn shortcut(&self, key: &Key, mac: bool) -> Option<Action> {
        let screen = self.screen.as_ref()?;
        screen.element.shortcuts.press_for(key, mac).cloned()
    }

    /// The screen's node `id`.
    pub(super) fn find(&self, id: ElementId) -> Option<&Node> {
        self.screen.as_ref().and_then(|node| node.find(id))
    }
}

impl Node {
    /// Whether Tab may give this node the focus: it takes keys, or a press on it does something
    /// now, as a browser's tab order holds fields and enabled buttons.
    pub(super) fn focusable(&self) -> bool {
        let element = &self.element;
        element.focus || (element.press.is_some() && element.enabled)
    }
}
