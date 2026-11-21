//! Tooltips (docs/beryl-3d.md, "Widgets"): words that explain the element the pointer rests
//! on. A tooltip is an element anywhere in the screen's tree, anchored `Anchor::Tip` and named
//! `tooltip <name>` for the element named `<name>` that it explains (`widgets::tooltip`). It is
//! hidden until the pointer has rested on that element for the milliseconds set by
//! `tooltip.delay_ms` in `flat.json`,
//! as the web interface's `.tip` waits for `:hover` (`ui/src/design/components/tips.css`),
//! and then stands centred above the element, or below it where there is no room above,
//! `tooltip.gap` points away. It takes no presses.

use glam::Vec2;

use super::{Layer, menu::fit};
use crate::element::{Anchor, Display};
use crate::input::{Rect, Size};
use kit_frame::frame::{Anchor as SceneAnchor, ElementId, Role};

/// What a tooltip's name starts with, before the name of the element it explains.
pub const TIP_PREFIX: &str = "tooltip ";

/// The tooltip of the element the pointer is on.
#[derive(Clone, Copy, Debug)]
pub(super) struct Resting {
    tooltip: ElementId,
    /// The id of the current target, whose box can move after the wait starts.
    target: ElementId,
    /// When the pointer came onto that element: the `now` of the first frame after it did.
    since: Option<u64>,
    shown: bool,
}

impl Layer {
    /// Takes where the pointer is, or `None` once it has left the window or a finger has
    /// lifted; the screen's elements that wait for the table (`Element::waits`) count only while it is `open`. A menu row it comes onto is highlighted
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
            self.hit(point, open, |node, _| {
                let tooltip = self.tooltip_of(&node.element.name)?;
                Some((tooltip, node.id))
            })
        });
        let current = self.tip.map(|tip| (tip.tooltip, tip.target));
        if wanted == current {
            return;
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
        if tip.shown || now.saturating_sub(since) < self.tuning.tooltip.delay_ns() {
            return;
        }
        tip.shown = true;
        let tooltip = tip.tooltip;
        self.show_tip(tooltip);
    }

    /// When the tooltip that is waiting will show, in nanoseconds since the client started, so
    /// that the client asks for a frame then; `None` with none waiting.
    #[must_use]
    pub fn tip_due(&self) -> Option<u64> {
        let tip = self.tip.as_ref().filter(|tip| !tip.shown)?;
        tip.since
            .map(|since| since.saturating_add(self.tuning.tooltip.delay_ns()))
    }

    /// Starts the tooltip's enter transition when its wait ends.
    fn show_tip(&mut self, id: ElementId) {
        let Some(node) = self.screen.as_mut().and_then(|node| node.find_mut(id)) else {
            return;
        };
        if let Some(enter) = node.element.style.enter {
            node.presence(enter, true);
        }
    }

    /// Places a visible tooltip against its target's current box, even when the
    /// tooltip is registered before its target in the screen.
    pub(super) fn tip_anchor(
        &self,
        id: ElementId,
        size: Vec2,
        open: bool,
    ) -> Option<(SceneAnchor, Rect)> {
        let tip = self.tip.filter(|tip| tip.shown && tip.tooltip == id)?;
        let (_, target) = self.positioned_node(tip.target, open)?;
        let anchor = beside(
            tip.target,
            target,
            size,
            self.window.size,
            self.tuning.menu_edge,
            self.tuning.tooltip.gap,
        );
        Some((anchor, target))
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

/// Centres a tooltip above its target, or below it when it would pass the
/// window's top. The horizontal offset keeps the tooltip inside the window.
fn beside(
    id: ElementId,
    target: Rect,
    size: Vec2,
    window: Size,
    edge: f32,
    gap: f32,
) -> SceneAnchor {
    let x = target.x + (target.width - size.x) / 2.0;
    let dx = fit(x, size.x, window.width, edge) - x;
    let above = target.y - gap - size.y;
    if above >= edge {
        SceneAnchor::new(
            id,
            Vec2::new(0.5, 0.0),
            Vec2::new(0.5, 1.0),
            Vec2::new(dx, -gap),
        )
    } else {
        SceneAnchor::new(
            id,
            Vec2::new(0.5, 1.0),
            Vec2::new(0.5, 0.0),
            Vec2::new(dx, gap),
        )
    }
}
