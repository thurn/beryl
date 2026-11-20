//! Draws the marks that `Lit` holds (docs/beryl-3d.md, "Objects and placement": marks are
//! separate quads). Each mark is the `marks` entry of the same name in
//! `data/ui-conventions.json`, drawn as the web interface draws it (`ui/src/stage/stage.css`,
//! `.stage-mark-*`):
//!
//! - eligible, selected and the two target marks are an outline round the card or ability, in
//!   the mark's color, that moves and turns with it: the stylesheet's four box shadows (a solid
//!   ring, two halos and a line inside the edge), each a `BoxShadow` record (`Outline`);
//! - suggested is a thin line in the ember color a little outside the card;
//! - paying draws no outline: the permanent turns as it would tap, and a tap symbol lies on its
//!   middle;
//! - a mark on a player is the outline round their life total.
//!
//! Which mark a thing wears is never decided here: the session's scripts and the prompt that is
//! open begin and end them.

use std::f32::consts::FRAC_PI_2;

use beryl_protocol::protocol::Side;
use beryl_protocol::script::{Curve as Named, Mark, Subject};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Lit, MarkSpan, Open, Span};
use crate::art::orb;
use crate::data::space::light;
use crate::element::Shadow;
use crate::frame::{
    Band, Draw, ElementId, FrameBuilder, Ring, Ringed, StateBits, box_shadow_reach, rows, sprites,
};
use crate::layout::shape_radius;
use crate::play::Scene;
use crate::pose::{Curve, Pose};
use crate::table::{ABILITY_IDS, CARD_HEIGHT, CARD_WIDTH, Look};
use crate::tokens::color;

/// How far through its coming and going an open mark is at `now`, from 0 (not seen) to 1
/// (whole). It comes up over the interval it began over and goes over the one it ended over.
/// Only `eligible` and `paying` take time to come up, and both ease out (the `mark` verb's
/// cases in `data/ui-conventions.json`); every other mark, and every end, takes none, so one
/// curve serves them all.
fn shown(open: &Open, now: u64) -> f32 {
    let through = |from: u64, to: u64| {
        if now >= to {
            1.0
        } else if now <= from {
            0.0
        } else {
            #[expect(clippy::cast_precision_loss, reason = "a fade's share of its time")]
            let share = (now - from) as f32 / (to - from) as f32;
            Curve::named(Named::EaseOut).ease(share)
        }
    };
    let up = through(open.begins.from, open.begins.to);
    let down = open.ends.map_or(0.0, |ends| through(ends.from, ends.to));
    up * (1.0 - down)
}

/// Whether the mark of `open` stands at `now`: from the moment it begins, the light coming up
/// from nothing, until it has gone. A mark whose end takes no time is gone as the end begins.
fn stands(open: &Open, now: u64) -> bool {
    now >= open.begins.from && open.ends.is_none_or(|ends| now < ends.to)
}

/// Changes how the card `on` is drawn at `now` for the marks on it in `lit`: `focused` while a
/// `dwell` step leaves it to be read (`Span::Focus`), the state bits of the marks it wears, and
/// for `paying` a quarter turn toward tapped as the mark comes up (`ui/src/stage/tracks.ts`,
/// `paying`), drawn a little above the cards it now reaches across.
/// A permanent the script has already tapped under the mark lies turned as it rests, and does
/// not turn again.
pub fn look(lit: &Lit, on: Subject, now: u64, look: &mut Look) {
    for open in lit.open() {
        let mark = match open.span {
            Span::Focus(focused) if focused == on && stands(open, now) => {
                look.state |= StateBits::FOCUSED;
                continue;
            }
            Span::Mark(MarkSpan { mark, on: marked }) if marked == on && stands(open, now) => mark,
            _ => continue,
        };
        match mark {
            Mark::Eligible => look.state |= StateBits::ELIGIBLE,
            Mark::Selected => look.state |= StateBits::SELECTED,
            Mark::FriendlyTarget | Mark::HostileTarget => look.state |= StateBits::TARGET,
            Mark::Suggested => {}
            Mark::Paying => {
                let turned = shown(open, now);
                if look.state & StateBits::TAPPED == 0 {
                    look.pose.rotation *= Quat::from_rotation_z(-FRAC_PI_2 * turned);
                }
                look.pose.position.z += light::PAYING_LIFT * turned;
            }
        }
    }
}

/// The band of a mark on `on` while it is at `pose`: `Band::TableMarks` for a card lying on
/// the table, below `light.on_table_below` millimetres, whose mark the cards over it hide, and
/// `Band::RaisedMarks` for anything raised (in flight, in a hand, on the stack), for an ability
/// and for a player.
#[must_use]
pub fn band(on: Subject, pose: &Pose) -> Band {
    if matches!(on, Subject::Object { .. }) && pose.position.z < light::ON_TABLE_BELOW {
        Band::TableMarks
    } else {
        Band::RaisedMarks
    }
}

/// The color of the outline or symbol of `mark`.
fn tint(mark: Mark) -> [u8; 4] {
    match mark {
        Mark::Eligible => color::MARK_ELIGIBLE,
        Mark::Selected => color::MARK_SELECTED,
        Mark::Suggested => color::MARK_SUGGESTED,
        Mark::Paying => color::MARK_PAYING,
        Mark::FriendlyTarget => color::MARK_FRIENDLY_TARGET,
        Mark::HostileTarget => color::MARK_HOSTILE_TARGET,
    }
}

/// Writes the records of the marks in `lit` as they are at `now`: for each, quads lying just
/// over the face of the thing it is on, so that they move and turn with it. Two marks on one
/// card or ability are both drawn over the same box, as the web's are; two on a player are
/// drawn the later begun outside the earlier. A quad over a card lying on
/// the table goes in `Band::TableMarks`, which the depth test of the cards drawn opaque before
/// it covers where another card lies over that one; a quad over anything raised (in flight, in
/// a hand, on the stack) or on a player goes in `Band::RaisedMarks`, and one over a card held
/// up in the tray in `Band::LayerCards`, with the card, over the tray's shade. Allocates
/// nothing.
pub fn write(lit: &Lit, now: u64, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    let open = lit.open();
    for (index, mark_open) in open.iter().enumerate() {
        let Span::Mark(MarkSpan { mark, on }) = mark_open.span else {
            continue;
        };
        let shown = shown(mark_open, now);
        if shown <= 0.0 {
            continue;
        }
        let Some(reach) = scene.reach(on) else {
            continue;
        };
        let pose = reach.pose;
        let [red, green, blue, alpha] = tint(mark);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an opacity from 0 to 255"
        )]
        let alpha = (f32::from(alpha) * shown.clamp(0.0, 1.0)).round() as u8;
        let tint = [red, green, blue, alpha];
        // Just over the face, along the face's own normal, so that a card held up toward the
        // camera has its mark in front of it too. A player's mark rings the core of their orb,
        // which lies a little off the middle of the orb's art, where their anchor is.
        let face = match on {
            Subject::Player { player } => player_face_offset(scene, player, pose.scale),
            Subject::Object { .. } | Subject::Ability { .. } => 0.0,
        };
        let position = pose.position + pose.rotation * Vec3::new(0.0, -face, light::LIFT);
        let band = match on {
            Subject::Object { slot } if scene.table().in_tray(usize::from(slot.0)) => {
                Band::LayerCards
            }
            _ => band(on, &pose),
        };
        if mark == Mark::Paying {
            let size = light::TAP_SIZE * pose.scale;
            // `look` lifts the paying card off the table as the mark comes up; the symbol
            // rises with it, or the card's face would hide it from the depth test.
            let transform = pose.sheared(Affine3A::from_scale_rotation_translation(
                Vec3::new(size, size, 1.0),
                pose.rotation,
                position + Vec3::Z * light::PAYING_LIFT * shown,
            ));
            builder.draw(band, Draw::glow(transform, tint, sprites::TAP, 0.0));
            continue;
        }
        if let (Some(outline), Subject::Object { .. } | Subject::Ability { .. }) =
            (Outline::of(mark), on)
        {
            let ring = |model, line, outer| Ring {
                on: ringed(on),
                mark,
                model,
                line,
                outer,
                tint,
                shadows: outline.shadows,
                frames: 0.0,
                radius: outline.radius,
            };
            outline.write(band, (&pose, position), reach.half, tint, ring, builder);
            continue;
        }
        let half = match on {
            Subject::Player { .. } => player_half(scene, pose.scale),
            Subject::Object { .. } | Subject::Ability { .. } => reach.half,
        };
        // Each outline begun earlier on the same thing, and still open, lies inside this one.
        let inside = open[..index]
            .iter()
            .filter(|earlier| {
                matches!(earlier.span, Span::Mark(MarkSpan { mark: other, on: marked })
                    if marked == on && other != Mark::Paying)
            })
            .count();
        #[expect(clippy::cast_precision_loss, reason = "a handful of marks")]
        let grow = light::OUTLINE_GROW * pose.scale * (1 + inside) as f32;
        let size = sprites::outline_size(half + Vec2::splat(grow));
        let transform = pose.sheared(Affine3A::from_scale_rotation_translation(
            size.extend(1.0),
            pose.rotation,
            position,
        ));
        let outline = Draw::glow(transform, tint, sprites::OUTLINE, 0.0);
        builder.ring(Ring {
            on: ringed(on),
            mark,
            model: outline.model,
            line: sprites::OUTLINE_LINE,
            outer: sprites::OUTLINE_OUTER,
            tint,
            shadows: &[],
            frames: 0.0,
            radius: 0.0,
        });
        builder.draw(band, outline);
    }
}

/// One layer of a mark's outline as `ui/src/stage/stage.css` writes it, centred on the box:
/// blurred by `blur` and grown by `spread`, both in frames (the stylesheet's `--frame`, a
/// card's width), in white at `opacity` out of 255, which the mark's color tints.
const fn layer(blur: f32, spread: f32, opacity: u8) -> Shadow {
    Shadow::new(0.0, 0.0, blur, [255, 255, 255, opacity]).spread(spread)
}

/// The `box-shadow` of `.stage-mark-eligible` and `.stage-mark-selected`: a solid ring, a halo
/// at 80%, a wider halo at 45% and a line inside the edge at 90%.
const LIT: [Shadow; 4] = [
    layer(0.0, 0.014, 255),
    layer(0.05, 0.03, 204),
    layer(0.14, 0.05, 115),
    layer(0.0, 0.012, 230).inset(),
];

/// The `box-shadow` of the two target marks, whose halo is wider and fuller than `LIT`'s: a
/// solid ring, a halo, a wider halo at 70% and a blurred line inside the edge.
const TARGET: [Shadow; 4] = [
    layer(0.0, 0.018, 255),
    layer(0.07, 0.045, 255),
    layer(0.2, 0.08, 179),
    layer(0.04, 0.014, 255).inset(),
];

/// `.stage-mark-suggested`'s solid border of 0.016 frames, as an inset shadow of that spread
/// inside its box.
const SUGGESTED: [Shadow; 1] = [layer(0.0, 0.016, 255).inset()];

/// A mark's outline as the web interface's `.stage-mark` element draws it: a box over the card
/// or ability, `grow` frames larger on every side (the element's negative `inset`), with
/// corners of `radius` frames, and its CSS box shadows.
struct Outline {
    grow: f32,
    radius: f32,
    shadows: &'static [Shadow],
}

impl Outline {
    /// The outline of `mark`, or none for `paying`, which draws a symbol and no outline.
    fn of(mark: Mark) -> Option<Self> {
        let (grow, radius, shadows): (f32, f32, &'static [Shadow]) = match mark {
            Mark::Eligible | Mark::Selected => (0.0, 0.05, &LIT),
            Mark::FriendlyTarget | Mark::HostileTarget => (0.0, 0.05, &TARGET),
            Mark::Suggested => (0.04, 0.08, &SUGGESTED),
            Mark::Paying => return None,
        };
        Some(Self {
            grow,
            radius,
            shadows,
        })
    }

    /// Writes one `BoxShadow` record for each of the outline's shadows round the thing `half`
    /// millimetres across and up from `position`, at `pose`, in the color `tint`, and its ring
    /// `ring(model, line, outer)`. The records lie in the card's own plane and units, a frame
    /// being `CARD_WIDTH`, and `pose` turns and scales them with it. The outer shadows come
    /// first, the inset ones after, each kind last first, as CSS paints them. Allocates
    /// nothing.
    fn write(
        &self,
        band: Band,
        (pose, position): (&Pose, Vec3),
        half: Vec2,
        tint: [u8; 4],
        ring: impl Fn([f32; 12], [f32; 2], [f32; 2]) -> Ring,
        builder: &mut FrameBuilder<'_>,
    ) {
        let frame = CARD_WIDTH;
        let scale = pose.scale.max(f32::EPSILON);
        let size = 2.0 * (half / scale + Vec2::splat(self.grow * frame));
        let radius = self.radius * frame;
        let placed = |quad: Vec2| pose.frame((quad * scale).extend(1.0), position);
        let mut farthest = 0.0_f32;
        for inset in [false, true] {
            for layer in self
                .shadows
                .iter()
                .rev()
                .filter(|layer| layer.inset == inset)
            {
                let [red, green, blue, _] = tint;
                let opacity = u16::from(layer.color[3]) * u16::from(tint[3]) / 255;
                let shadow = Shadow {
                    blur: layer.blur * frame,
                    spread: layer.spread * frame,
                    color: [red, green, blue, u8::try_from(opacity).unwrap_or(u8::MAX)],
                    ..*layer
                };
                let reach = box_shadow_reach(&shadow);
                farthest = farthest.max(reach);
                let spread = if inset { -shadow.spread } else { shadow.spread };
                builder.draw(
                    band,
                    Draw::box_shadow(
                        placed(size + Vec2::splat(2.0 * reach)),
                        shadow.color,
                        size,
                        radius,
                        &shadow,
                        shape_radius(size, radius, spread),
                    ),
                );
            }
        }
        let outer = Vec2::splat(0.5) + Vec2::splat(farthest) / size.max(Vec2::splat(f32::EPSILON));
        let mut record = ring(rows(placed(size)), [0.5, 0.5], outer.to_array());
        record.frames = size.x / frame;
        builder.ring(record);
    }
}

/// What an outline on `on` rings, as the table names it: a card's element id is its slot, and
/// an ability's is `ABILITY_IDS` more than its id (`table::scene`).
fn ringed(on: Subject) -> Ringed {
    match on {
        Subject::Object { slot } => Ringed::Element(ElementId(u32::from(slot.0))),
        Subject::Ability { id } => Ringed::Element(ElementId(ABILITY_IDS + id % ABILITY_IDS)),
        Subject::Player { player } => Ringed::Player(player),
    }
}

/// Half the width and height, in millimetres, of a player's life total at the depth of their
/// anchor, whose pose has `scale`: the face of the web interface's life orb
/// (`light.player_half_pixels`, in pixels of its 1600 by 900 stage). The anchor's pose is scaled
/// so that a card would fill the hand's share of the window's height
/// (`Camera::screen_pose`), which gives how many millimetres there one pixel of the stage's
/// 900 covers.
pub(crate) fn player_half(scene: &Scene<'_>, scale: f32) -> Vec2 {
    let window = scale * CARD_HEIGHT / scene.table().layout().hand.height;
    light::PLAYER_HALF_PIXELS * (window / 900.0)
}

/// How far below their anchor, in millimetres at the depth of the anchor, whose pose has
/// `scale`, the middle of `side`'s life total is: the core of their orb lies
/// `art::orb::face_offset` units of the orb's art below the art's middle, which the anchor
/// holds (`hud::players::face_centre`).
fn player_face_offset(scene: &Scene<'_>, side: Side, scale: f32) -> f32 {
    let table = scene.table();
    let window = table.camera().window;
    let points = orb::face_offset(side) * orb::SCALES[usize::from(window.phone())];
    let millimetres_per_point =
        scale * CARD_HEIGHT / table.layout().hand.height / window.size.height;
    points * millimetres_per_point
}
