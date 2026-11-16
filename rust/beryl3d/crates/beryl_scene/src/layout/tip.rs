//! Tooltips (docs/beryl-3d.md, "Widgets"): words that explain the element the pointer rests
//! on. A tooltip is an element anywhere in the screen's tree, anchored `Anchor::Tip` and named
//! `tooltip <name>` for the element named `<name>` that it explains (`widgets::tooltip`). It is
//! hidden until the pointer has rested on that element for `flat.tooltip_delay` milliseconds,
//! as the web interface's `.tip` waits for `:hover` (`ui/src/design/components/tips.css`),
//! and then stands centred above the element, or below it where there is no room above,
//! `flat.tooltip_gap` points away. It takes no presses.

use glam::Vec2;

use super::{Layer, menu::fit};
use crate::data::space::flat::{MENU_EDGE, TOOLTIP_DELAY, TOOLTIP_GAP};
use crate::element::{Anchor, Display};
use crate::frame::{ElementId, Role};
use crate::input::{Rect, Size};

/// What a tooltip's name starts with, before the name of the element it explains.
pub const TIP_PREFIX: &str = "tooltip ";

/// How long the pointer rests on an element before its tooltip shows, in nanoseconds.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
const DELAY: u64 = TOOLTIP_DELAY as u64 * 1_000_000;

/// The tooltip of the element the pointer is on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Resting {
    tooltip: ElementId,
    /// The box on the window of the element it explains.
    target: Rect,
    /// When the pointer came onto that element: the `now` of the first frame after it did.
    since: Option<u64>,
    shown: bool,
}

impl Layer {
    /// Takes where the pointer is, or `None` once it has left the window or a finger has
    /// lifted; the screen's elements count only while it is `open`. A menu row it comes onto is highlighted
    /// (`menu`), and the tooltip of the element it comes onto starts to wait; the next frame
    /// starts the wait's clock (`tick`).
    pub fn hover(&mut self, point: Option<Vec2>, open: bool) {
        let row = point.and_then(|point| {
            self.hit(point, open, |node, _| {
                (node.element.role == Role::MenuItem).then_some(node.id)
            })
        });
        if row != self.pointed {
            self.pointed = row;
            if let Some(row) = row {
                self.point_at_row(row);
            }
        }
        let wanted = point.and_then(|point| {
            self.hit(point, open, |node, origin| {
                let tooltip = self.tooltip_of(&node.element.name)?;
                Some((tooltip, node.rect_at(origin)))
            })
        });
        let now = self.tip.map(|tip| tip.tooltip);
        if wanted.map(|(tooltip, _)| tooltip) == now {
            return;
        }
        if let Some(old) = now {
            self.show_tip(old, None);
        }
        self.tip = wanted.map(|(tooltip, target)| Resting {
            tooltip,
            target,
            since: None,
            shown: false,
        });
    }

    /// Starts the clock of a tooltip that began to wait since the last frame, and shows the
    /// tooltip whose wait is over. Called by `tick` on every frame; allocates nothing.
    pub(super) fn tick_tip(&mut self, now: u64) {
        let Some(tip) = &mut self.tip else {
            return;
        };
        let since = *tip.since.get_or_insert(now);
        if tip.shown || now.saturating_sub(since) < DELAY {
            return;
        }
        tip.shown = true;
        let (tooltip, target) = (tip.tooltip, tip.target);
        self.show_tip(tooltip, Some(target));
    }

    /// When the tooltip that is waiting will show, in nanoseconds since the client started, so
    /// that the client asks for a frame then; `None` with none waiting.
    #[must_use]
    pub fn tip_due(&self) -> Option<u64> {
        let tip = self.tip.as_ref().filter(|tip| !tip.shown)?;
        tip.since.map(|since| since + DELAY)
    }

    /// Shows the tooltip `id` beside `target`, or hides it with `None`.
    fn show_tip(&mut self, id: ElementId, target: Option<Rect>) {
        let window = self.window.size;
        let Some(node) = self.screen.as_mut().and_then(|node| node.find_mut(id)) else {
            return;
        };
        node.shown = target.is_some();
        if let Some(target) = target {
            let size = Vec2::new(node.rect.width, node.rect.height);
            node.placed = Some(beside(target, size, window));
        }
    }

    /// The tooltip on the screen that explains the element named `name`, if any.
    fn tooltip_of(&self, name: &str) -> Option<ElementId> {
        fn search(node: &super::Node, name: &str) -> Option<ElementId> {
            if node.element.style.display == Display::None {
                return None;
            }
            let element = &node.element;
            if matches!(element.anchor, Some(Anchor::Tip))
                && element.name.strip_prefix(TIP_PREFIX) == Some(name)
            {
                return Some(node.id);
            }
            node.children.iter().find_map(|child| search(child, name))
        }
        if name.is_empty() {
            return None;
        }
        self.screen.as_ref().and_then(|node| search(node, name))
    }
}

/// Where a tooltip `size` large stands beside the element whose box is `target`, on a window
/// of `window`'s size: centred above it, or below it where it would pass the window's top,
/// and kept inside the window across.
fn beside(target: Rect, size: Vec2, window: Size) -> Vec2 {
    let x = target.x + (target.width - size.x) / 2.0;
    let above = target.y - TOOLTIP_GAP - size.y;
    let y = if above >= MENU_EDGE {
        above
    } else {
        target.y + target.height + TOOLTIP_GAP
    };
    Vec2::new(fit(x, size.x, window.width), y)
}
