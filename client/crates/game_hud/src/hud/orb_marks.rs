//! Paints the player marks on the life-orb art, under the life totals.

use game_play::art::orb::face_id;
use game_play::light::{Lit, marks};
use game_table::table::Table;
use glam::Vec2;
use kit_base::data::profiles::Piece;
use kit_base::geometry::Rect;
use kit_base::geometry::plane::ScreenPlane;
use kit_frame::frame::text::faded;
use kit_frame::frame::{
    AnchorBox, Band, Draw, ElementId, FrameBuilder, MARK_IDS, Missing, NameRef, NodeRef, Place,
    Resolved, Ring, Role, ShapeAnchor, StateBits, outline_corners, rows,
};
use kit_ui::widgets::icons::Icons;

use crate::art::orb;

/// Draws the same cushion and rim for both seats in the orb's artwork units. The art and the
/// mark share the layout's anchor and scale, including when the phone raises the viewer's orb.
pub fn write(
    lit: &Lit,
    now: u64,
    table: &Table,
    plane: &ScreenPlane,
    icons: &Icons,
    builder: &mut FrameBuilder<'_>,
) {
    if !table.profile().data().hud.table.contains(&Piece::Players) {
        return;
    }
    let units = table.layout().play.life.orb_scale;
    let camera = table.camera();
    let line = [0.5, 0.5];
    let outer =
        (Vec2::splat(0.5) + Vec2::splat(orb::MARK_GLOW_REACH) / (2.0 * orb::MARK_HALF)).to_array();
    for (index, side, mark, tint) in marks::player_faces(lit, now) {
        let face = table.life_centre(side) + Vec2::Y * orb::face_offset(side) * units;
        let bounds = AnchorBox {
            centre: face,
            half: orb::MARK_HALF * units,
        };
        let model = rows(plane.layer_transform(bounds.rect()));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "active marks fit the mark id range"
        )]
        let id = ElementId(MARK_IDS + index as u32);
        let element = builder.resolved(
            mark.name(),
            Resolved {
                id,
                parent: u32::MAX,
                role: Role::Mark,
                name: NameRef::default(),
                place: Place::Layer,
                pose: None,
                corners: outline_corners(model, line, camera.view_projection, camera.window.size),
                depth: 0.0,
                opacity: f32::from(tint[3]) / f32::from(u8::MAX),
                clip: None,
                interactive: false,
                state: StateBits(StateBits::PASSES),
                face: None,
                count: 1,
                annotations: Default::default(),
            },
        );
        let anchor = face_id(side);
        if builder.anchor_bounds(&NodeRef::Id(anchor)).is_none() {
            let core = AnchorBox {
                centre: face,
                half: Vec2::new(orb::CORE_RX, orb::CORE_RY) * units,
            };
            builder.anchor_node(anchor, core.rect());
        }
        builder.decorate(element, ShapeAnchor::Outline(anchor));
        builder.ring(Ring {
            element,
            name: mark.name(),
            model,
            line,
            outer,
            tint,
            shadows: &[],
            frames: 0.0,
            radius: 0.0,
        });

        for (cell, alpha) in orb::mark_layers() {
            let Some(sprite) = icons.get(cell.icon, cell.size) else {
                builder.undrawn(
                    element,
                    Missing::Icon {
                        icon: cell.icon.0,
                        size: cell.size,
                    },
                );
                continue;
            };
            let [_, _, width, height] = cell.view;
            let size = units * Vec2::new(width, height);
            let at = Rect {
                x: face.x - size.x / 2.0,
                y: face.y - size.y / 2.0,
                width: size.x,
                height: size.y,
            };
            let draw = Draw::icon(
                plane.layer_transform(at),
                faded(tint, f32::from(alpha) / f32::from(u8::MAX)),
                sprite,
            );
            builder.draw(Band::Layer, draw);
        }
    }
}
