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

use beryl_protocol::script::{Mark, Subject};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Lit, MarkSpan, Open, Span};
use crate::play::{LightTarget, Scene};
use game_table::table::Look;
use kit_base::data::light::Light;
use kit_base::pose::{Curve, Pose};
use kit_base::tokens::color;
use kit_frame::frame::{
    Band, Draw, ElementId, FrameBuilder, NameRef, Place, Resolved, Ring, Role, ShapeAnchor,
    StateBits, box_shadow_reach, outline_corners, rows, sprites,
};
use kit_ui::element::Shadow;
use kit_ui::layout::shape_radius;

/// Only eligible and paying marks fade in. Both use the same ease-out curve.
fn shown(open: &Open, now: u64) -> f32 {
    kit_space::motion::visibility(open.begins, open.ends, Curve::EASE_OUT, now)
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
pub fn look(light: &Light, lit: &Lit, on: Subject, now: u64, look: &mut Look) {
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
                look.pose.position.z += light.paying_lift * turned;
            }
        }
    }
}

/// The resting pass for a mark. Marks on cards lying close to the table are tested against
/// the cards above them. The rest share the pass used for hand cards and stack items.
#[must_use]
pub fn band(light: &Light, on: Subject, pose: &Pose) -> Band {
    if matches!(on, Subject::Object { .. }) && pose.position.z < light.on_table_below {
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
/// drawn the later begun outside the earlier. A mark on an object shares its paint band,
/// so it follows the object in flight, in a screen preview, or under the pointer.
pub fn write(lit: &Lit, now: u64, scene: &Scene<'_>, builder: &mut FrameBuilder<'_>) {
    let light = &scene.table().data().light;
    let open = lit.open();
    for (index, mark_open) in open.iter().enumerate() {
        let Span::Mark(MarkSpan { mark, on }) = mark_open.span else {
            continue;
        };
        let shown = shown(mark_open, now);
        if shown <= 0.0 {
            continue;
        }
        let Some(target) = scene.light_target(on, builder) else {
            continue;
        };
        let reach = target.reach;
        let pose = reach.pose;
        let [red, green, blue, alpha] = tint(mark);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an opacity from 0 to 255"
        )]
        let alpha = (f32::from(alpha) * shown.clamp(0.0, 1.0)).round() as u8;
        let tint = [red, green, blue, alpha];
        let position = pose.position + pose.rotation * Vec3::Z * light.lift;
        let band = target.band.records(band(light, on, &pose));
        let camera = scene.table().camera();
        let node = MarkNode {
            index,
            mark,
            target,
            shown,
            camera,
        };
        let clip = target
            .element
            .and_then(|(_, element)| element.clip)
            .and_then(|rect| {
                builder.scissor(kit_base::geometry::zoom_rect(
                    rect,
                    (Vec2::ZERO, camera.window.scale),
                ))
            })
            .unwrap_or(0);
        if mark == Mark::Paying {
            let size = light.tap_size * pose.scale;
            // The symbol keeps its orientation while the paying card turns under it.
            let was_tapped = target
                .element
                .is_some_and(|(_, node)| node.state.has(StateBits::TAPPED));
            let rotation = if was_tapped {
                pose.rotation
            } else {
                pose.rotation * Quat::from_rotation_z(FRAC_PI_2 * shown)
            };
            let transform = pose.sheared(Affine3A::from_scale_rotation_translation(
                Vec3::new(size, size, 1.0),
                rotation,
                position,
            ));
            let draw = Draw::glow(transform, tint, sprites::TAP, 0.0).with_clip(clip);
            node.register(draw.model, [0.5, 0.5], builder);
            builder.draw(band, draw);
            continue;
        }
        if let (Some(outline), Subject::Object { .. } | Subject::Ability { .. }) =
            (Outline::of(mark), on)
        {
            outline.write((band, clip), position, tint, &node, builder);
            continue;
        }
        // Each outline begun earlier on the same thing, and still open, lies inside this one.
        let inside = open[..index]
            .iter()
            .filter(|earlier| {
                matches!(earlier.span, Span::Mark(MarkSpan { mark: other, on: marked })
                    if marked == on && other != Mark::Paying)
            })
            .count();
        #[expect(clippy::cast_precision_loss, reason = "a handful of marks")]
        let grow = light.outline_grow * pose.scale * (1 + inside) as f32;
        let size = sprites::outline_size(reach.half + Vec2::splat(grow));
        let transform = pose.sheared(Affine3A::from_scale_rotation_translation(
            size.extend(1.0),
            pose.rotation,
            position,
        ));
        let outline = Draw::glow(transform, tint, sprites::OUTLINE, 0.0);
        let element = node.register(outline.model, sprites::OUTLINE_LINE, builder);
        builder.ring(Ring {
            element,
            mark,
            model: outline.model,
            line: sprites::OUTLINE_LINE,
            outer: sprites::OUTLINE_OUTER,
            tint,
            shadows: &[],
            frames: 0.0,
            radius: 0.0,
        });
        builder.draw(band, outline.with_clip(clip));
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

    /// Writes one record per shadow in the card's own plane and units, with the same clip
    /// as the card. The outer shadows come first, then the inset ones, as CSS paints them.
    fn write(
        &self,
        (band, clip): (Band, u8),
        position: Vec3,
        tint: [u8; 4],
        node: &MarkNode<'_>,
        builder: &mut FrameBuilder<'_>,
    ) {
        let frame = builder.card().x;
        let pose = node.target.reach.pose;
        let scale = pose.scale.max(f32::EPSILON);
        let size = 2.0 * (node.target.reach.half / scale + Vec2::splat(self.grow * frame));
        let radius = self.radius * frame;
        let placed = |quad: Vec2| pose.frame((quad * scale).extend(1.0), position);
        let model = rows(placed(size));
        let line = [0.5, 0.5];
        let element = node.register(model, line, builder);
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
                    )
                    .with_clip(clip),
                );
            }
        }
        let outer = Vec2::splat(0.5) + Vec2::splat(farthest) / size.max(Vec2::splat(f32::EPSILON));
        builder.ring(Ring {
            element,
            mark: node.mark,
            model,
            line,
            outer: outer.to_array(),
            tint,
            shadows: self.shadows,
            frames: size.x / frame,
            radius: self.radius,
        });
    }
}

/// A mark and the target it follows in the current frame.
struct MarkNode<'a> {
    index: usize,
    mark: Mark,
    target: LightTarget,
    shown: f32,
    camera: &'a game_table::table::Camera,
}

impl MarkNode<'_> {
    fn register(&self, model: [f32; 12], line: [f32; 2], builder: &mut FrameBuilder<'_>) -> u32 {
        let pose = self.target.reach.pose;
        let (parent, place, clip, state) = self
            .target
            .element
            .map_or((u32::MAX, Place::Layer, None, 0), |(index, target)| {
                (index, target.place, target.clip, target.state.0)
            });
        #[expect(
            clippy::cast_possible_truncation,
            reason = "active marks fit the mark id range"
        )]
        let id = ElementId((1 << 24) + self.index as u32);
        let at = builder.resolved(
            self.mark.name(),
            Resolved {
                id,
                parent,
                role: Role::Mark,
                name: NameRef::default(),
                place,
                pose: Some(pose),
                corners: outline_corners(
                    model,
                    line,
                    self.camera.view_projection,
                    self.camera.window.size,
                ),
                depth: self.camera.depth(pose.position),
                opacity: self.shown,
                clip,
                interactive: false,
                state: StateBits(
                    StateBits::PASSES
                        | state
                            & (StateBits::MOVING
                                | StateBits::HELD
                                | StateBits::SHOWN
                                | StateBits::LAYOUT_MOTION),
                ),
                face: None,
                copies: 1,
                really: NameRef::default(),
                note: NameRef::default(),
            },
        );
        let id = self.target.id;
        let anchor = if self.mark == Mark::Paying {
            ShapeAnchor::Symbol(id)
        } else {
            ShapeAnchor::Outline(id)
        };
        builder.decorate(at, anchor);
        at
    }
}
