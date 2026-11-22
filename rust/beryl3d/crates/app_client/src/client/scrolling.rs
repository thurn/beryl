//! Routes the toolkit's scroll offsets to the flat layer or to a line of cards. Flat
//! offsets move the viewer's card boxes with their grid. Group offsets go through the
//! table's placement, which also supplies the counts at each end.

use glam::Vec2;

use super::{
    Client,
    input::{Node, ScenePoint},
};
use app_screens::screens::Screen;
use game_hud::hud::rows::Message as RowMessage;
use game_play::pointer::rows::scroller_of;
use game_table::table::{Scroller, Table};
use kit_base::geometry::{GroupWindow, ScrollAxis};
use kit_frame::frame::{ElementId, Role};

use kit_scene::scroll::ScrollInput;
use kit_ui::input::{Button, Key, PointerKind, PointerPhase};
use kit_ui::layout::Layer;
use kit_ui::scroll::Viewport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Element(ElementId),
    Row(Scroller),
    Tray(usize),
}

impl Client {
    fn rows_scroll(&self) -> bool {
        matches!(self.features.screen, Screen::Game | Screen::Table)
            && self.features.sheet.is_none()
            && self.features.viewer().open().is_none()
            && self.features.menu().is_none()
            && !self.features.overlay_open()
            && !self.table_frozen()
    }

    pub(super) fn scroll_pointer(&mut self, point: &ScenePoint) -> bool {
        let pointer = &point.pointer;
        let rows = self.rows_scroll();
        let at = match pointer.phase {
            PointerPhase::Down if pointer.button == Button::Primary => {
                self.scroll_target(point, rows, None)
            }
            PointerPhase::Wheel => self.scroll_target(point, rows, Some(pointer.wheel)),
            _ => None,
        };
        if pointer.phase == PointerPhase::Down
            && let Some((Target::Row(row), _)) = at
        {
            self.features.focus_row(RowMessage::Focus(Some(row)));
            self.layer.focus(None);
        }
        // A card that has picked up keeps its drag. Scroll and pickup use the same
        // recognized direction when they share a press.
        if rows && pointer.phase == PointerPhase::Move && self.show.pointer().held().is_some() {
            self.routes.scrolling.cancel();
        }
        let view = |target| viewport(target, &self.layer, self.show.table(), rows);
        let ScrollInput::Claimed(update) = self.routes.scrolling.pointer(
            pointer,
            point.drag.map(|drag| drag.direction),
            self.now,
            at,
            view,
            &self.show.table().data().pointer,
        ) else {
            return false;
        };
        if pointer.phase != PointerPhase::Wheel {
            self.suppress_press();
            if self.layer.edited_id().is_some() {
                self.edit(kit_ui::layout::Edit::Up);
            }
            self.layer.point_at(None, None);
            if !self.bug_open() {
                self.show.pointer_mut().cancel();
            }
        }
        if let Some((target, offset)) = update {
            self.apply_scroll(target, offset);
        }
        true
    }

    fn scroll_target(
        &self,
        routed: &ScenePoint,
        rows: bool,
        wheel: Option<Vec2>,
    ) -> Option<(Target, Viewport)> {
        let pointer = &routed.pointer;
        let point = pointer.position;
        if let Some((id, view)) = self.layer.scroll_target_at(
            point,
            self.features.layer_open,
            wheel,
            self.data().pointer.wheel_notch_points,
        ) {
            // Let mouse drags select text inside a scrollable sheet.
            if wheel.is_none()
                && pointer.kind == PointerKind::Mouse
                && self.layer.field_at(point, self.features.layer_open)
            {
                return None;
            }
            return Some((Target::Element(id), view));
        }
        if !rows
            || routed
                .target
                .is_some_and(|target| matches!(target.node, Node::Control(_)))
        {
            return None;
        }
        if let Some((index, viewport)) = self.show.table().tray_viewport_at(point)
            && wheel.is_none_or(|delta| {
                viewport.takes_wheel(delta, self.data().pointer.wheel_notch_points)
            })
        {
            if wheel.is_none()
                && pointer.kind == PointerKind::Mouse
                && let Some(kit_scene::input::Target {
                    node: Node::Card(slot),
                    container: Some(place),
                }) = routed.target
                && self.holdable(slot, place)
            {
                return None;
            }
            return Some((Target::Tray(index), viewport));
        }
        let element = &self.frame.resolved[routed.hit? as usize];
        let row = scroller_of(element.place).filter(|_| element.role == Role::Card)?;
        if self.show.table().in_tray(element.id.0 as usize) {
            return None;
        }
        let view = row_viewport(self.show.table(), row)?;
        Some((Target::Row(row), view))
    }

    pub(super) fn scroll_key(&mut self, key: Key) -> bool {
        let step = self.data().pointer.scroll.key_step_points;
        let update = self
            .layer
            .scroll_key(key, self.features.layer_open, step)
            .map(|(id, offset)| (Target::Element(id), offset))
            .or_else(|| {
                let row = self
                    .features
                    .rows()
                    .focus()
                    .filter(|_| self.layer.focused().is_none() && self.rows_scroll())?;
                let offset = row_viewport(self.show.table(), row)?.key(key, step)?;
                Some((Target::Row(row), offset))
            });
        let Some((target, offset)) = update else {
            return false;
        };
        self.routes.scrolling.cancel();
        self.apply_scroll(target, offset);
        true
    }

    /// Samples momentum before drawing. A new group in view can require script placement.
    pub(crate) fn prepare_scroll(&mut self, now: u64) {
        let rows = self.rows_scroll();
        let view = |target| viewport(target, &self.layer, self.show.table(), rows);
        if let Some((target, offset)) =
            self.routes
                .scrolling
                .advance(now, view, &self.show.table().data().pointer)
        {
            self.apply_scroll(target, offset);
        }
    }

    fn apply_scroll(&mut self, target: Target, offset: Vec2) {
        match target {
            Target::Element(id) => {
                if self.layer.set_scroll_offset(id, offset) {
                    self.place_shown();
                }
            }
            Target::Tray(grid) => self.show.scroll_tray(grid, offset.y),
            Target::Row(row) => {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "group offsets are exact and bounded by a u16 card count"
                )]
                let first = offset.x as isize;
                if self.show.scroll_row(row, first) {
                    self.prepare_rows();
                }
            }
        }
    }
}

fn viewport(target: Target, layer: &Layer, table: &Table, rows: bool) -> Option<Viewport> {
    match target {
        Target::Element(id) => layer.scroll_viewport(id),
        Target::Row(row) if rows => row_viewport(table, row),
        Target::Tray(grid) if rows => table.tray_viewport(grid),
        Target::Row(_) | Target::Tray(_) => None,
    }
}

fn row_viewport(table: &Table, row: Scroller) -> Option<Viewport> {
    let window = table.row_window(row);
    window.scrolls().then(|| {
        Viewport::groups(GroupWindow {
            first: window.first(),
            shown: window.shown(),
            last: window.last(),
            axis: ScrollAxis::Horizontal,
        })
    })
}
