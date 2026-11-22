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
use super::super::tray_layout::{Held, TrayGrid, landing_at};
use super::SceneObject;

impl Table {
    /// The scrolling grid at the pointer, and its viewport.
    #[must_use]
    pub fn tray_viewport_at(&self, point: Vec2) -> Option<(usize, Viewport)> {
        self.tray
            .grids
            .iter()
            .enumerate()
            .find_map(|(index, grid)| {
                let point_box = Rect {
                    x: point.x,
                    y: point.y,
                    width: 0.0,
                    height: 0.0,
                };
                self.tray_viewport(index)
                    .filter(|_| contains(grid.region, point_box))
                    .map(|viewport| (index, viewport))
            })
    }

    /// A tray grid's offset and range, through the same recognizer as other scrollers.
    #[must_use]
    pub fn tray_viewport(&self, index: usize) -> Option<Viewport> {
        let grid = self.tray.grids.get(index).filter(|grid| grid.limit > 0.0)?;
        Some(Viewport::points(
            Vec2::Y * grid.offset,
            Vec2::Y * grid.limit,
            Vec2::new(grid.region.width, grid.region.height),
        ))
    }

    /// Sets the grid's offset. The clipping rectangle stays in place.
    pub fn scroll_tray(&mut self, index: usize, offset: f32) {
        if let Some(grid) = self.tray.grids.get_mut(index) {
            grid.offset = offset.clamp(0.0, grid.limit);
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
        landing_at(
            held,
            self.layout().tray,
            self.camera().window.size,
            point,
            carried,
            |index| self.tray.grids.get(index).map_or(0.0, |grid| grid.offset),
        )
    }

    fn tray_grid(&self, slot: usize) -> Option<&TrayGrid> {
        let index = self
            .tray
            .poses
            .iter()
            .position(|(card, _)| usize::from(card.0) == slot)?;
        self.tray
            .grids
            .iter()
            .find(|grid| grid.cards.contains(&index))
    }

    /// Shifts the tray destination with the scroll offset. An item entering a box that
    /// fits wholly inside the viewport can fly in from outside it.
    pub(super) fn tray_grid_rest(&self, slot: usize, object: &mut SceneObject) -> Option<Rect> {
        let grid = self.tray_grid(slot).filter(|grid| grid.limit > 0.0)?;
        self.camera()
            .move_in_screen(&mut object.pose, -Vec2::Y * grid.offset);
        self.tray_clip(slot)
    }

    pub(super) fn clip_tray_card(&self, object: &mut SceneObject, clip: Option<Rect>) {
        if let Some(clip) = clip {
            object.visible &= intersection(self.grid_box(object.pose), clip).is_some();
        }
    }

    pub(in crate::table) fn tray_clip(&self, slot: usize) -> Option<Rect> {
        let grid = self.tray_grid(slot).filter(|grid| grid.limit > 0.0)?;
        if self.tray.share(self.tray.now).is_some() {
            let (_, mut pose) = self.tray.poses[grid.cards.clone()]
                .iter()
                .find(|(card, _)| usize::from(card.0) == slot)
                .copied()?;
            self.camera()
                .move_in_screen(&mut pose, -Vec2::Y * grid.offset);
            if contains(grid.region, self.grid_box(pose)) {
                return None;
            }
        }
        Some(grid.region)
    }

    fn grid_box(&self, pose: kit_base::pose::Pose) -> Rect {
        kit_scene::hit::corner_bounds(self.camera().card_corners(pose.transform()))
    }

    /// Declares which part of each grid shows each scene object. The table draws and
    /// animates the object itself, so these boxes take no pointer input.
    pub(in crate::table) fn write_tray_grids(&self, builder: &mut FrameBuilder) {
        for (index, grid) in self.tray.grids.iter().enumerate() {
            let id =
                ElementId(TRAY_GRID_IDS + u32::try_from(index).expect("grid index fits in u32"));
            let clip = (grid.limit > 0.0).then_some(grid.region);
            let parent = builder.resolved(&grid.name, region(id, u32::MAX, grid.region, None));
            for (&(slot, mut pose), label) in
                self.tray.poses[grid.cards.clone()].iter().zip(&grid.labels)
            {
                // A screen viewer owns the picture while it also lists a card held in a tray.
                if self.in_box(usize::from(slot.0)) {
                    continue;
                }
                if grid.offset > 0.0 {
                    self.camera()
                        .move_in_screen(&mut pose, -Vec2::Y * grid.offset);
                }
                let rect = self.grid_box(pose);
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
        copies: 0,
        really: NameRef::default(),
        note: NameRef::default(),
    }
}
