//! The shown boxes and scroll viewports of game-prompt trays.

use beryl_protocol::protocol::ObjectId;
use glam::Vec2;
use kit_base::geometry::Rect;
use kit_frame::frame::{
    ElementId, FrameBuilder, NameRef, Place, Resolved, Role, StateBits, TRAY_CARD_BOX_IDS,
    TRAY_GRID_IDS,
};
use kit_scene::hit::{contains, intersection};
use kit_ui::scroll::Viewport;

use super::super::Table;
use super::super::tray_layout::{Held, TrayGrid};
use super::SceneObject;

impl Table {
    /// A tray grid's offset and range, through the same recognizer as other scrollers.
    #[must_use]
    pub fn tray_viewport(&self, index: usize) -> Option<Viewport> {
        let view = &self.tray.arrangement.grids.get(index)?.view;
        let clip = view.clip()?;
        Some(
            Viewport::points(
                Vec2::Y * view.offset(),
                Vec2::Y * view.limit(),
                Vec2::new(clip.width, clip.height),
            )
            .directional(),
        )
    }

    /// Sets the grid's offset. The clipping rectangle stays in place.
    pub fn scroll_tray(&mut self, index: usize, offset: f32) {
        if let Some(grid) = self.tray.arrangement.grids.get_mut(index) {
            grid.view.scroll_to(offset);
        }
    }

    /// Where a dragged card lands in the tray, accounting for the viewport's scroll.
    #[must_use]
    pub fn tray_landing(
        &self,
        held: &Held,
        point: Vec2,
        carried: ObjectId,
    ) -> Option<(usize, usize)> {
        // The drop command uses the prompt's pile indices. They must still describe the tray
        // that supplied the visible cards.
        if self.held.as_ref()? != held {
            return None;
        }
        self.tray.arrangement.landing(held, point, carried)
    }

    /// The grid containing this card in the tray.
    #[must_use]
    pub fn tray_grid_index(&self, slot: usize) -> Option<usize> {
        let index = self
            .tray
            .poses
            .iter()
            .position(|(card, _)| usize::from(card.0) == slot)?;
        self.tray
            .arrangement
            .grids
            .iter()
            .position(|grid| grid.cards.contains(&index))
    }

    fn tray_grid(&self, slot: usize) -> Option<&TrayGrid> {
        self.tray.arrangement.grids.get(self.tray_grid_index(slot)?)
    }

    /// Shifts the tray destination with the scroll offset. An item entering a box that
    /// fits wholly inside the viewport can fly in from outside it.
    pub(super) fn tray_grid_rest(&self, slot: usize, object: &mut SceneObject) -> Option<Rect> {
        let grid = self.tray_grid(slot)?;
        grid.view.clip()?;
        object.pose = grid.view.pose(self.camera(), object.pose);
        self.tray_clip(slot)
    }

    pub(super) fn clip_tray_card(&self, object: &mut SceneObject, clip: Option<Rect>) {
        if let Some(clip) = clip {
            object.visible &= intersection(self.grid_box(object.pose), clip).is_some();
        }
    }

    pub(in crate::table) fn tray_clip(&self, slot: usize) -> Option<Rect> {
        let grid = self.tray_grid(slot)?;
        let clip = grid.view.clip()?;
        if self.tray.share(self.tray.now).is_some() {
            let (_, pose) = self.tray.poses[grid.cards.clone()]
                .iter()
                .find(|(card, _)| usize::from(card.0) == slot)
                .copied()?;
            if contains(clip, self.grid_box(grid.view.pose(self.camera(), pose))) {
                return None;
            }
        }
        Some(clip)
    }

    fn grid_box(&self, pose: kit_base::pose::Pose) -> Rect {
        kit_scene::hit::corner_bounds(self.camera().card_corners(pose.transform()))
    }

    /// Declares which part of each grid shows each scene object. The table draws and
    /// animates the object itself, so these boxes take no pointer input.
    pub(in crate::table) fn write_tray_grids(&self, builder: &mut FrameBuilder) {
        for (index, grid) in self.tray.arrangement.grids.iter().enumerate() {
            let id =
                ElementId(TRAY_GRID_IDS + u32::try_from(index).expect("grid index fits in u32"));
            let clip = grid.view.clip();
            let parent =
                builder.resolved(&grid.name, region(id, u32::MAX, grid.view.region(), None));
            for (&(slot, pose), label) in
                self.tray.poses[grid.cards.clone()].iter().zip(&grid.labels)
            {
                // A screen viewer owns the picture while it also lists a card held in a tray.
                if self.in_box(usize::from(slot.0)) {
                    continue;
                }
                let rect = self.grid_box(grid.view.pose(self.camera(), pose));
                let id = ElementId(u32::from(slot.0));
                let shown = builder.resolved(
                    label,
                    region(ElementId(TRAY_CARD_BOX_IDS + id.0), parent, rect, clip),
                );
                builder.shows(shown, id);
            }
        }
    }
}

fn region(id: ElementId, parent: u32, rect: Rect, clip: Option<Rect>) -> Resolved {
    let low = Vec2::new(rect.x, rect.y);
    let high = low + Vec2::new(rect.width, rect.height);
    Resolved {
        id,
        parent,
        role: Role::Region,
        name: NameRef::default(),
        place: Place::Layer,
        pose: None,
        corners: [
            low,
            Vec2::new(high.x, low.y),
            high,
            Vec2::new(low.x, high.y),
        ],
        depth: 0.0,
        opacity: 1.0,
        clip,
        interactive: false,
        state: StateBits(StateBits::PASSES),
        face: None,
        count: 0,
        annotations: Default::default(),
    }
}
