//! Contact shadows (docs/beryl-3d.md, "The Baseline tier": shadows are single quads). A card
//! above what lies under it (in flight, in a hand, on the stack, held up) casts one soft shadow
//! on the table, at the height of the cards lying there, as the web interface's dragged card
//! does (`ui/src/stage/heft.ts`): the light hangs above the far left of the table, so a shadow
//! falls toward the viewer and a little right, the farther the higher the card, and it spreads,
//! softens and comes in with height. A card lying on the table casts none.

use beryl_protocol::protocol::ZoneName;
use glam::{Affine3A, Vec2, Vec3};

use super::pile::pile_base;
use super::scene::Table;
use super::{CARD_HEIGHT, CARD_WIDTH};
use crate::data::space::shadow;
use crate::frame::{Band, Draw, FrameBuilder, Place};
use crate::pose::Pose;

/// Half a card's diagonal, in millimetres at a scale of 1: no point of a card lies farther from
/// its centre.
const HALF_DIAGONAL: f32 = 54.1;

impl Table {
    /// Writes the contact shadow of the card in `slot` (none for an ability) drawn at `pose`,
    /// resting at `place`, if it stands more than `shadow.least_height` above what lies under
    /// it. A card at rest in a battlefield row, or in its pile, lies on the table: `under_way`
    /// says the card is moving, which no card lying still is.
    pub(super) fn write_shadow(
        &self,
        (slot, place): (Option<usize>, Place),
        pose: &Pose,
        under_way: bool,
        builder: &mut FrameBuilder,
    ) {
        if pose.position.z <= shadow::LEAST_HEIGHT || (!under_way && self.lies(place, pose)) {
            return;
        }
        let centre = pose.position;
        let under = self.floor(centre.truncate(), slot);
        let falls = |base: f32| centre.truncate() + light() * (centre.z - base);
        let base = under.max(self.floor(falls(under), slot));
        let height = centre.z - base;
        if height <= shadow::LEAST_HEIGHT {
            return;
        }
        // The card, grown by the shadow's spread in its own plane, and then cast along the
        // light onto the plane just above what lies under it.
        let spread = shadow::PENUMBRA * height;
        let scale = pose.scale.max(f32::EPSILON);
        let grown = Vec3::new(
            1.0 + spread / (CARD_WIDTH * scale),
            1.0 + spread / (CARD_HEIGHT * scale),
            1.0,
        );
        let card = pose.transform() * Affine3A::from_scale(grown);
        let transform = cast(base) * card;
        let coming_in = (height / shadow::EMERGING).min(1.0);
        let alpha = shadow::ALPHA * coming_in * pose.opacity.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "0 to 255"
        )]
        let alpha = (alpha * 255.0).round() as u8;
        // The blur is in the card mesh's own millimetres: the shadow fades over its spread.
        let blur = spread / (2.0 * scale * grown.x);
        builder.draw(
            Band::ContactShadows,
            Draw::contact_shadow(transform, [0, 0, 0, alpha], blur),
        );
    }

    /// Whether a card at `pose`, resting at `place`, lies on the table: in a battlefield row, or
    /// on its pile (and not lifted off it, beside the pile, while the opponent looks at it).
    fn lies(&self, place: Place, pose: &Pose) -> bool {
        let Place::Card { zone, side, .. } = place else {
            return false;
        };
        match zone {
            ZoneName::Battlefield => return true,
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile => {}
            _ => return false,
        }
        let pile = pile_base(self.layout(), self.camera(), side, zone);
        pose.position.truncate().distance_squared(pile) < 0.01
    }

    /// The height of what lies at `point` on the table: the highest card lying on the table at
    /// rest (in a pile or a battlefield row) whose face covers it, leaving out the card in
    /// `slot`; 0 where none does.
    fn floor(&self, point: Vec2, slot: Option<usize>) -> f32 {
        let mut floor = 0.0_f32;
        for (index, object) in self.objects.cards.iter().enumerate() {
            let Some(object) = object else {
                continue;
            };
            let Place::Card { zone, .. } = object.place else {
                continue;
            };
            let lying = matches!(
                zone,
                ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile | ZoneName::Battlefield
            );
            let pose = object.pose();
            if !lying || slot == Some(index) || pose.position.z <= floor {
                continue;
            }
            let offset = point - pose.position.truncate();
            let reach = HALF_DIAGONAL * pose.scale;
            if offset.x.abs() > reach || offset.y.abs() > reach {
                continue;
            }
            let local = pose.rotation.inverse() * offset.extend(0.0) / pose.scale;
            if local.x.abs() <= CARD_WIDTH / 2.0 && local.y.abs() <= CARD_HEIGHT / 2.0 {
                floor = pose.position.z;
            }
        }
        floor
    }
}

/// How far a shadow falls on the table for each millimetre of height: toward the viewer (down
/// the table's y) and a little to the right.
fn light() -> Vec2 {
    Vec2::new(shadow::LIGHT.x, -shadow::LIGHT.y)
}

/// Casts a point along the light onto the plane `shadow.above` millimetres over `base`.
fn cast(base: f32) -> Affine3A {
    let light = light();
    Affine3A::from_cols(
        Vec3::X.into(),
        Vec3::Y.into(),
        Vec3::new(light.x, light.y, 0.0).into(),
        Vec3::new(-light.x * base, -light.y * base, base + shadow::ABOVE).into(),
    )
}
