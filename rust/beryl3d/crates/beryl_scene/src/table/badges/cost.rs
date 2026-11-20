//! A hand card's mana cost, standing above its top right corner and turned with the card, so
//! that a crowded fan still shows every card's cost (UI Conventions, "The hand's fan"). It
//! follows the web interface's `.hand-cost` (`ui/src/game/Look.tsx`, styled in
//! `ui/src/screens/styles/card-layer.css`): a row of mana symbols, each a disc in its color with
//! the symbol's glyph on it and a dark ring, whose bottoms meet the frame's top, drawn in front
//! of the next card of the fan, whose turned corner reaches up over this card's.
//!
//! Each symbol is shaped once, the first time a face list names it (`Badges::shape_costs`), so a
//! frame only places glyphs it already has and allocates nothing. Its sizes are the web's, in
//! the type unit of a hand card (a twelfth of its width), from `badges.cost` in
//! `data/beryl3d/space.json`.

use beryl_protocol::protocol::{ObjectView, ZoneName};
use glam::{Affine3A, Quat, Vec2, Vec3};

use super::{Badges, On, faded};
use crate::data::space::badges::cost as data;
use crate::faces::{FaceList, ListedFace};
use crate::frame::{
    Draw, ElementId, FrameBuilder, NameRef, Place, RECORDS_PER_OBJECT, Resolved, Role, Sprite,
    StateBits,
};
use crate::text::fonts::Font;
use crate::text::{Block, Glyphs, Text, TypeStyle, glyph_key};
use crate::tokens::{badge, mana};

use super::super::{CARD_HEIGHT, CARD_WIDTH, Camera};
use crate::light::arrows::ARROW_IDS;

/// The element id of the cost of the card in slot 0: past the arrows' and the aims'. The cost
/// of the card in slot `s` is this plus `s`.
const COST_IDS: u32 = ARROW_IDS + (1 << 17);

/// The style a symbol is shaped in: the card font around it, for a symbol the Mana font has no
/// glyph for, at a size near the one a hand card shows it at, so that its glyph is drawn from a
/// cell of about its size.
const STYLE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 700,
    italic: false,
    size: data::POINTS,
    line_height: 1.0,
    letter_spacing: 0.0,
};

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

/// The millimetres in one type unit of a hand card.
fn em() -> f32 {
    data::EM * CARD_WIDTH
}

impl Badges {
    /// Shapes every mana symbol of a cost in `list` not shaped yet, and asks `wanted` for their
    /// glyphs. A list's costs are shaped when its first view arrives; one already shaped costs
    /// a look-up for each of its symbols.
    pub fn shape_costs(&mut self, text: &mut Text, wanted: &mut Glyphs, list: &FaceList) {
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
                let block = text.block(&cost_of(symbol), STYLE, &[], None).clone();
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

    /// Writes the cost of `object`, a card in a hand shown face up with `face`, above the top
    /// right corner of the card `on` describes, and its resolved element as a child of the
    /// card's, at `parent` in `Frame::resolved`, where the card rests at `place`. A card with no cost, or whose symbols are not
    /// shaped, writes nothing, and a cost whose records would pass `BUDGET` is cut after the
    /// last symbol that fits.
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
        let (disc, gap) = (data::SYMBOL * em(), data::GAP * em());
        #[expect(
            clippy::cast_precision_loss,
            reason = "a cost has far fewer than 2^24 symbols"
        )]
        let width = shown as f32 * disc + (shown - 1) as f32 * gap;
        let corner = Vec2::new(CARD_WIDTH, CARD_HEIGHT) / 2.0;
        let right = corner.x - data::RIGHT * em();
        let bottom = corner.y - data::SINK * em();
        // Toward the eye by `LIFT` millimetres, in the card's own units, so that the cost stands
        // in front of the next cards of the fan.
        let height = data::LIFT / on.pose.scale.max(f32::EPSILON);
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
                data::RING * em(),
            ),
        );
        // Millimetres per point of the shaped symbol, and how far its glyphs stand above the disc.
        let k = disc / symbol.rect.width;
        let raise = super::data::RAISE * super::em();
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
