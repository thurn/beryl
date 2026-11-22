//! Draws each mana symbol as a ringed disc above the hand card's upper-right corner, turned with
//! the card, so the next card in the fan leaves the cost visible. This follows the web's
//! `.hand-cost` (UI Conventions, "The hand's fan"). `badges.cost` measures each disc and gap in
//! twelfths of a hand card's width. `Badges::shape_costs` shapes symbols when a face list arrives.
//! A frame places the cached glyphs without allocating.
//! The sizes come from `cost` in `data/beryl3d/badges.json`.

use beryl_protocol::protocol::{ObjectView, ZoneName};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Badges, On, faded};
use crate::faces::{FaceList, ListedFace};
use kit_base::data::BadgeTuning;
use kit_base::tokens::{badge, mana};
use kit_frame::frame::{
    Draw, ElementId, FrameBuilder, NameRef, Place, RECORDS_PER_OBJECT, Resolved, Role, Sprite,
    StateBits,
};
use kit_text::text::fonts::Font;
use kit_text::text::{Block, Glyphs, Text, TypeStyle, glyph_key};

use super::super::Camera;
use kit_frame::frame::ARROW_IDS;

/// The element id of the cost of the card in slot 0: past the arrows' and the aims'. The cost
/// of the card in slot `s` is this plus `s`.
const COST_IDS: u32 = ARROW_IDS + (1 << 17);

/// The style a symbol is shaped in: the card font around it, for a symbol the Mana font has no
/// glyph for, at a size near the one a hand card shows it at, so that its glyph is drawn from a
/// cell of about its size.
fn style(tuning: &BadgeTuning) -> TypeStyle {
    TypeStyle {
        font: Font::CrimsonPro,
        weight: 700,
        italic: false,
        size: tuning.cost.points,
        line_height: 1.0,
        letter_spacing: 0.0,
    }
}

/// The records a cost may write: those a card may write besides itself, its soft edge, its
/// contact shadow and a mark. A hand card wears no badges, so they are all the cost's: seven
/// symbols of one glyph each, or a generic symbol and three hybrid ones, whose letters take
/// three glyphs each.
const BUDGET: usize = RECORDS_PER_OBJECT - 4;

/// The shaped mana symbols, by the text between their braces, sorted by it.
#[derive(Debug, Default)]
pub(super) struct Costs {
    symbols: Vec<(Box<str>, Block)>,
}

impl Costs {
    fn get(&self, symbol: &str) -> Option<&Block> {
        let at = self
            .symbols
            .binary_search_by(|(shaped, _)| (**shaped).cmp(symbol))
            .ok()?;
        Some(&self.symbols[at].1)
    }
}

/// The text between the braces of each symbol of `cost`, such as `2` and `U` in `{2}{U}`, as
/// the text shaper finds them (`text::symbols::tokens`), with no allocation, since every hand
/// card reads its cost on every frame.
fn symbols(cost: &str) -> impl Iterator<Item = &str> {
    cost.split('}')
        .filter_map(|part| part.rsplit_once('{').map(|(_, symbol)| symbol))
        .filter(|symbol| !symbol.is_empty())
}

impl Badges {
    /// The millimetres in one type unit of a hand card.
    fn cost_em(&self) -> f32 {
        self.sizes().tuning.cost.em * self.sizes().card.width
    }

    /// Shapes every mana symbol of a cost in `list` not shaped yet, and asks `wanted` for their
    /// glyphs. A list's costs are shaped when its first view arrives; one already shaped costs
    /// a look-up for each of its symbols.
    pub fn shape_costs(&mut self, text: &mut Text, wanted: &mut Glyphs, list: &FaceList) {
        let Some(sizes) = self.sizes else {
            return;
        };
        let costs = list
            .faces()
            .iter()
            .filter_map(|face| face.text.mana_cost.as_deref());
        for cost in costs {
            for symbol in symbols(cost) {
                let Err(at) = self
                    .costs
                    .symbols
                    .binary_search_by(|(shaped, _)| (**shaped).cmp(symbol))
                else {
                    continue;
                };
                let block = text
                    .block(&cost_of(symbol), style(&sizes.tuning), &[], None)
                    .clone();
                wanted.want(&block, self.scale);
                self.costs.symbols.insert(at, (symbol.into(), block));
            }
        }
    }

    /// Asks `wanted` for the glyphs of every shaped symbol at the current scale.
    pub(super) fn want_costs(&self, wanted: &mut Glyphs) {
        for (_, block) in &self.costs.symbols {
            wanted.want(block, self.scale);
        }
    }

    /// Writes the cost of a face-up hand card over its upper-right corner and registers it
    /// under the card's node. Omits unshaped costs and stops before a symbol exceeds `BUDGET`.
    pub(in crate::table) fn write_cost(
        &self,
        (object, face, parent, place): (&ObjectView, Option<&ListedFace>, u32, Place),
        (on, camera): (&On, &Camera),
        builder: &mut FrameBuilder,
    ) {
        if object.zone != ZoneName::Hand || on.pose.turn > 0.5 {
            return;
        }
        let Some(cost) = face.and_then(|face| face.text.mana_cost.as_deref()) else {
            return;
        };
        let blocks = || symbols(cost).map_while(|symbol| self.costs.get(symbol));
        let mut left = BUDGET;
        let shown = blocks()
            .take_while(|block| {
                let records = 1 + block.glyphs.len();
                let fits = records <= left;
                left = left.saturating_sub(records);
                fits
            })
            .count();
        if shown == 0 {
            return;
        }
        let (disc, gap) = (
            self.tuning().cost.symbol * self.cost_em(),
            self.tuning().cost.gap * self.cost_em(),
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "a cost has far fewer than 2^24 symbols"
        )]
        let width = shown as f32 * disc + (shown - 1) as f32 * gap;
        let corner = self.sizes().card.half();
        let right = corner.x - self.tuning().cost.right * self.cost_em();
        let bottom = corner.y - self.tuning().cost.sink * self.cost_em();
        // Toward the eye by `LIFT` millimetres, in the card's own units, so that the cost stands
        // in front of the next cards of the fan.
        let height = self.tuning().cost.lift / on.pose.scale.max(f32::EPSILON);
        let card = on.pose.upward();
        let box_corners = [
            Vec2::new(right - width, bottom),
            Vec2::new(right, bottom),
            Vec2::new(right, bottom + disc),
            Vec2::new(right - width, bottom + disc),
        ]
        .map(|at| camera.to_screen(card.transform_point3(at.extend(height))));
        builder.resolved(
            cost,
            Resolved {
                id: ElementId(COST_IDS + u32::from(object.slot.0)),
                parent,
                role: Role::Text,
                name: NameRef::default(),
                place,
                pose: Some(kit_base::pose::Pose {
                    position: card.transform_point3(Vec3::new(
                        right - width / 2.0,
                        bottom + disc / 2.0,
                        height,
                    )),
                    turn: on.pose.turn - on.pose.turn.round(),
                    ..on.pose
                }),
                corners: box_corners,
                depth: camera.depth(card.transform_point3(Vec3::new(0.0, 0.0, height))),
                opacity: on.opacity,
                clip: None,
                interactive: false,
                // A press goes through the cost to the card under it, as the web's
                // `.hand-cost` takes no pointer events.
                state: StateBits(StateBits::PASSES),
                face: None,
                copies: 1,
                really: NameRef::default(),
                note: NameRef::default(),
            },
        );
        let mut x = right - width;
        for block in blocks().take(shown) {
            let at = Affine3A::from_translation(Vec3::new(x, bottom + disc, height));
            self.write_symbol(block, (card * at, disc), on, builder);
            x += disc + gap;
        }
    }

    /// Writes one symbol of a cost, shaped as `block`: its disc, `disc` across with its top
    /// left corner at the origin of `corner`, which lies on the card's face, and its glyphs on
    /// the disc.
    fn write_symbol(
        &self,
        block: &Block,
        (corner, disc): (Affine3A, f32),
        on: &On,
        builder: &mut FrameBuilder,
    ) {
        let Some(symbol) = block.symbols.first() else {
            return;
        };
        let quad = |at: Vec2, size: Vec2, height: f32| {
            corner
                * Affine3A::from_scale_rotation_translation(
                    size.extend(1.0),
                    Quat::IDENTITY,
                    at.extend(height),
                )
        };
        builder.draw(
            on.band,
            Draw::panel(
                quad(Vec2::new(disc, -disc) / 2.0, Vec2::splat(disc), 0.0),
                faded(symbol.color, on.opacity),
                faded(badge::COST_RING, on.opacity),
                Vec2::splat(disc),
                disc / 2.0,
                self.tuning().cost.ring * self.cost_em(),
            ),
        );
        // Millimetres per point of the shaped symbol, and how far its glyphs stand above the disc.
        let k = disc / symbol.rect.width;
        let raise = self.raise();
        for glyph in &block.glyphs {
            let run = block.run(glyph.run);
            let Some((placed, larger)) =
                self.glyphs
                    .nearest(glyph_key(run.style, self.scale, glyph.glyph))
            else {
                continue;
            };
            let points = larger / self.scale;
            let [_, _, w, h] = placed.rect;
            let size = Vec2::new(f32::from(w), f32::from(h)) * points * k;
            let from_disc = Vec2::new(
                glyph.x + f32::from(placed.bearing[0]) * points - symbol.rect.x,
                glyph.y + f32::from(placed.bearing[1]) * points - symbol.rect.y,
            );
            let at = Vec2::new(
                from_disc.x * k + size.x / 2.0,
                -from_disc.y * k - size.y / 2.0,
            );
            builder.draw(
                on.band,
                Draw::glyph(
                    quad(at, size, raise),
                    faded(run.color.unwrap_or(mana::INK), on.opacity),
                    Sprite {
                        page: placed.page,
                        rect: placed.rect,
                    },
                ),
            );
        }
    }
}

/// `symbol` in braces, as the text shaper reads a mana symbol.
fn cost_of(symbol: &str) -> String {
    format!("{{{symbol}}}")
}

#[cfg(test)]
mod tests {
    use super::symbols;

    #[test]
    fn symbols_are_read_between_braces() {
        let read = |cost| symbols(cost).collect::<Vec<_>>();
        assert_eq!(read("{2}{W}{U}"), ["2", "W", "U"]);
        assert_eq!(read("{1}{R/W}{R/W}"), ["1", "R/W", "R/W"]);
        assert_eq!(read(""), Vec::<&str>::new());
    }
}
