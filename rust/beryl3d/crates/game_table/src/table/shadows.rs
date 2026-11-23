//! Contact shadows (docs/beryl-3d.md, "The Baseline tier": shadows are single quads). A card
//! above what lies under it (in flight, in a hand, on the stack, held up) casts one soft shadow
//! on the table, at the height of the cards lying there, as the web interface's dragged card
//! does (`ui/src/stage/heft.ts`): the light hangs above the far left of the table, so a shadow
//! falls toward the viewer and a little right, the farther the higher the card, and it spreads,
//! softens and comes in with height. A card lying on the table casts none.

use beryl_protocol::protocol::ZoneName;
use glam::{Affine3A, Vec2, Vec3};

use super::pile::PileShown;
use super::scene::{SceneObject, Table};
use crate::table::Place;
use kit_base::data::Card;
use kit_base::data::lighting::Shadow;
use kit_base::pose::Pose;
use kit_frame::frame::{Band, Draw, FrameBuilder};
use kit_ui::input::Tier;

impl Table {
    /// Writes the contact shadow of the card in `slot` (none for an ability) drawn at `pose`,
    /// resting at `place`, if it stands more than `contact.least_height` (in
    /// `data/beryl3d/lighting.json`) above what lies under it. A card at rest in a battlefield row, or in its pile, lies on the table: `under_way`
    /// says the card is moving, which no card lying still is.
    pub(super) fn write_shadow(
        &self,
        (slot, place): (Option<usize>, Place),
        pose: &Pose,
        under_way: bool,
        builder: &mut FrameBuilder,
    ) {
        // The Enhanced tier's shadow map shades every card's shadow, so it writes no contact
        // shadows (docs/beryl-3d.md, "The Enhanced tier").
        if builder.tier() == Tier::Enhanced {
            return;
        }
        let shadow = &self.data().lighting.shadow;
        let contact = &shadow.contact;
        // A card a box of the flat layer shows casts no shadow on the table under the box.
        let boxed = slot.is_some_and(|slot| self.in_box(slot));
        if boxed
            || pose.position.z <= contact.least_height
            || (!under_way && self.lies(place, pose, contact.pile_tolerance_squared))
        {
            return;
        }
        let centre = pose.position;
        let under = self.floor(centre.truncate(), slot);
        let falls = |base: f32| centre.truncate() + shadow.fall() * (centre.z - base);
        let base = under.max(self.floor(falls(under), slot));
        let height = centre.z - base;
        if height <= contact.least_height {
            return;
        }
        // The card, grown by the shadow's spread in its own plane, and then cast along the
        // light onto the plane just above what lies under it.
        let spread = contact.penumbra * height;
        let scale = pose.scale.max(f32::EPSILON);
        let grown = Vec3::new(
            1.0 + spread / (self.camera().data.card.width * scale),
            1.0 + spread / (self.camera().data.card.height * scale),
            1.0,
        );
        let card = pose.transform() * Affine3A::from_scale(grown);
        let transform = cast(shadow, base) * card;
        let coming_in = (height / contact.emerging).min(1.0);
        let alpha = shadow.darkness * coming_in * pose.opacity.clamp(0.0, 1.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "0 to 255"
        )]
        let alpha = (alpha * f32::from(u8::MAX)).round() as u8;
        // The blur is in the card mesh's own millimetres: the shadow fades over its spread.
        let blur = spread / (2.0 * scale * grown.x);
        builder.draw(
            Band::ContactShadows,
            Draw::contact_shadow(transform, [0, 0, 0, alpha], blur),
        );
    }

    /// Whether a card at `pose`, resting at `place`, lies in a battlefield row or on its pile.
    fn lies(&self, place: Place, pose: &Pose, pile_tolerance_squared: f32) -> bool {
        let Place::Card { zone, side, row } = place else {
            return false;
        };
        // A card tucked beneath a permanent shares that permanent's row, even if it is in
        // exile. On a tilted row its centre can be higher than the permanent's centre, which
        // would cast a narrow shadow across both faces if the card were treated as raised.
        if row.is_some() || zone == ZoneName::Battlefield {
            return true;
        }
        match zone {
            ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile => {}
            _ => return false,
        }
        let pile = self.objects.piles[PileShown::index(side, zone)].base;
        pose.position.truncate().distance_squared(pile) < pile_tolerance_squared
    }

    /// The height of what lies at `point` on the table: the highest card lying on the table at
    /// rest (in a pile or a battlefield row) whose face covers it, leaving out the card in
    /// `slot`; 0 where none does. Only the cards of `Objects::floors` whose centres lie within
    /// reach of `point` are looked at.
    fn floor(&self, point: Vec2, slot: Option<usize>) -> f32 {
        let floors = &self.objects.floors;
        let mut floor = 0.0_f32;
        if floors.lying.is_empty() {
            return floor;
        }
        let (low, high) = (point.x - floors.reach, point.x + floors.reach);
        for band in floors.band(point.y - floors.reach)..=floors.band(point.y + floors.reach) {
            let from = floors
                .lying
                .partition_point(|lying| (lying.band, lying.x) < (band, low));
            for lying in &floors.lying[from..] {
                if lying.band != band || lying.x > high {
                    break;
                }
                if slot == Some(lying.slot as usize) || lying.z <= floor {
                    continue;
                }
                let offset = point - Vec2::new(lying.x, lying.y);
                let reach = floors.half.length() * lying.scale;
                if offset.x.abs() > reach || offset.y.abs() > reach {
                    continue;
                }
                let local = lying.inverse * offset.extend(0.0) / lying.scale;
                if local.x.abs() <= floors.half.x && local.y.abs() <= floors.half.y {
                    floor = lying.z;
                }
            }
        }
        floor
    }
}

/// The cards lying on the table at rest, in a pile or a battlefield row, for `Table::floor`:
/// sorted by the band across the table their centre lies in, each band as tall as `reach`, and
/// within one by the x of their centre. Every card that rises or moves looks for what lies under
/// its shadow twice a frame, and looking through every card each time made a frame with 255
/// cards in flight cost eight times what it did before shadows. Made with the scene objects,
/// when a view or a script arrives, so a frame allocates nothing for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Floors {
    lying: Vec<Lying>,
    /// The farthest any of their faces reaches from its centre, in millimetres.
    reach: f32,
    /// Half a card's printed width and height, in millimetres at a scale of 1.
    half: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Lying {
    band: i32,
    x: f32,
    y: f32,
    z: f32,
    scale: f32,
    inverse: glam::Quat,
    slot: u32,
}

impl Floors {
    /// The cards of `cards`, by slot, that lie on the table, for cards of `card`'s size.
    pub(super) fn of(cards: &[Option<SceneObject>], card: Card) -> Self {
        // Half a card's diagonal, in millimetres at a scale of 1: no point of a card lies
        // farther from its centre.
        let half_diagonal = card.half().length();
        let lies = |object: &&SceneObject| {
            matches!(
                object.place,
                Place::Card {
                    zone: ZoneName::Library
                        | ZoneName::Graveyard
                        | ZoneName::Exile
                        | ZoneName::Battlefield,
                    ..
                }
            )
        };
        let reach = (cards.iter().flatten().filter(lies))
            .map(|object| half_diagonal * object.pose.scale)
            .fold(0.0_f32, f32::max);
        let mut lying: Vec<Lying> = (cards.iter().enumerate())
            .filter_map(|(slot, object)| {
                let pose = object.as_ref().filter(lies)?.pose;
                Some(Lying {
                    band: band(pose.position.y, reach),
                    x: pose.position.x,
                    y: pose.position.y,
                    z: pose.position.z,
                    scale: pose.scale,
                    inverse: pose.rotation.inverse(),
                    slot: u32::try_from(slot).expect("slots are u16"),
                })
            })
            .collect();
        lying.sort_unstable_by(|a, b| a.band.cmp(&b.band).then(a.x.total_cmp(&b.x)));
        Floors {
            lying,
            reach,
            half: card.half(),
        }
    }

    /// The band across the table that `y` lies in.
    fn band(&self, y: f32) -> i32 {
        band(y, self.reach)
    }
}

/// The band across the table that `y` lies in, when each is `reach` tall.
fn band(y: f32, reach: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a table is far narrower than 2^31 bands"
    )]
    let band = (y / reach.max(1.0)).floor() as i32;
    band
}

/// Casts a point along `shadow`'s light onto the plane `contact.above` millimetres over `base`.
fn cast(shadow: &Shadow, base: f32) -> Affine3A {
    let light = shadow.fall();
    Affine3A::from_cols(
        Vec3::X.into(),
        Vec3::Y.into(),
        Vec3::new(light.x, light.y, 0.0).into(),
        Vec3::new(
            -light.x * base,
            -light.y * base,
            base + shadow.contact.above,
        )
        .into(),
    )
}
