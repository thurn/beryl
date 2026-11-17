//! The badges a permanent wears on its face, and a hand card's mana cost (`cost`)
//! (docs/beryl-3d.md, "Objects and placement": marks are separate quads). A permanent wears its
//! power and toughness on a plaque at the bottom right, or its loyalty there for a
//! planeswalker; a badge for each kind of counter on it, along its foot; and, on the top card of
//! a pile of identical permanents, how many the pile holds. They follow the web interface's
//! `Card` (`ui/src/components/Card.tsx`): a plaque's numbers are inked up or down against the
//! printed ones (`toneClass`), and the toughness shown is less the damage marked on the
//! creature, inked down, as `frameFace` in `ui/src/game/faces.ts` draws it.
//!
//! Each badge is a rounded panel lying on the card's face, which moves and turns with the card,
//! and its number is glyph records. Its sizes are the web's, in the em of a battlefield card
//! (a tenth of its width), from `badges` in `data/beryl3d/space.json`. The digits and signs are
//! shaped once, when the first view arrives (`Badges::shape`), so a frame only places glyphs it
//! already has and allocates nothing.

use beryl_protocol::protocol::{Counter, ObjectView, ZoneName};
use glam::{Affine3A, Quat, Vec2};

use super::{CARD_HEIGHT, CARD_WIDTH};
use crate::data::space::badges as data;
use crate::faces::ListedFace;
use crate::frame::{Band, Draw, FrameBuilder, RECORDS_PER_OBJECT, Sprite};
use crate::input::GlyphCell;
use crate::pose::Pose;
use crate::text::fonts::Font;
use crate::text::{Block, Glyphs, Text, TypeStyle, glyph_key};
use crate::tokens::{badge, color};

mod cost;

/// The style every badge's number is shaped in: the card font's bold, at the size in points a
/// plaque's numerals have on a battlefield card of the web interface.
pub const STYLE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 700,
    size: data::NUMERAL_POINTS,
    line_height: 1.0,
};

/// The highest number a badge shows; a higher one shows as this.
const MOST: u32 = 99;
/// The pieces of text a badge is made of: the digits 0 to 9 at their own indices, then the
/// signs. `+`, at 10, is shaped for the badges of later marks. Shaping the digits alone, not
/// each number to `MOST`, costs a tenth of the text runs and allocations when the first view
/// arrives; the card font's numerals are all one width, so a number set digit by digit reads
/// as one shaped whole would.
const MINUS: usize = 11;
const SLASH: usize = 12;
const TIMES: usize = 13;
const PIECES: usize = 14;

/// The records a permanent's badges may write: enough for a plaque of single-digit power and
/// toughness (a panel and three glyphs) and two single-digit counter badges. A card may write
/// more besides itself, its soft edge, its contact shadow and a mark (`RECORDS_PER_OBJECT`), for
/// a hand card's cost, whose hybrid symbols take more records than a permanent's badges.
const BUDGET: usize = 8;
const _: () = assert!(BUDGET <= RECORDS_PER_OBJECT - 4);
/// The most pieces of text one badge holds: `-12/-34`.
const LONGEST: usize = 7;

/// One millimetre of the web card's em, a tenth of a battlefield card's width.
fn em() -> f32 {
    data::EM * CARD_WIDTH
}

/// The shaped numbers and signs badges are made of, and where the glyph atlas holds their
/// glyphs.
#[derive(Debug, Default)]
pub struct Badges {
    /// By piece: the digits, then `+`, `-`, `/` and `×`. Empty until shaped.
    pieces: Vec<Block>,
    glyphs: Glyphs,
    /// Physical pixels per logical point, which picks the glyphs' size in the atlas.
    scale: f32,
    /// The mana symbols of hand cards' costs (`cost`).
    costs: cost::Costs,
}

impl Badges {
    /// Whether the pieces are shaped.
    #[must_use]
    pub fn shaped(&self) -> bool {
        !self.pieces.is_empty()
    }

    /// Shapes every piece with `text`, once, and asks `wanted` for their glyphs at `scale`.
    pub fn shape(&mut self, text: &mut Text, wanted: &mut Glyphs, scale: f32) {
        self.scale = scale;
        if self.pieces.is_empty() {
            let pieces = [
                "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "+", "-", "/", "\u{d7}",
            ];
            self.pieces = pieces
                .into_iter()
                .map(|piece| text.block(piece, STYLE, &[], None).clone())
                .collect();
            debug_assert_eq!(self.pieces.len(), PIECES);
        }
        self.want(wanted);
    }

    /// Takes a new number of physical pixels per logical point, and asks `wanted` for the
    /// glyphs at that size.
    pub fn rescale(&mut self, scale: f32, wanted: &mut Glyphs) {
        self.scale = scale;
        self.want(wanted);
    }

    /// Records the glyphs of an atlas page that is ready.
    pub fn add(&mut self, page: u16, cells: &[GlyphCell]) {
        self.glyphs.add(page, cells);
    }

    fn want(&self, wanted: &mut Glyphs) {
        for block in &self.pieces {
            wanted.want(block, self.scale);
        }
        self.want_costs(wanted);
    }
}

/// How a number reads against the one printed on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Printed,
    Up,
    Down,
}

impl Tone {
    fn of(now: i32, printed: Option<i32>) -> Self {
        match printed.map(|printed| now.cmp(&printed)) {
            Some(std::cmp::Ordering::Greater) => Self::Up,
            Some(std::cmp::Ordering::Less) => Self::Down,
            _ => Self::Printed,
        }
    }

    /// The ink of a number on the pale plaque (`.card-pt b` and `toneClass`).
    fn ink(self) -> [u8; 4] {
        match self {
            Self::Printed => badge::PLAQUE_INK,
            Self::Up => color::STAT_UP_INK,
            Self::Down => color::STAT_DOWN_INK,
        }
    }
}

/// A badge's text: up to `LONGEST` pieces, each with its ink.
#[derive(Clone, Copy, Default)]
struct Label {
    pieces: [(usize, [u8; 4]); LONGEST],
    len: usize,
}

impl Label {
    fn push(&mut self, piece: usize, ink: [u8; 4]) {
        if self.len < LONGEST {
            self.pieces[self.len] = (piece, ink);
            self.len += 1;
        }
    }

    /// `value` as pieces, with a minus sign when it is below zero.
    fn number(&mut self, value: i32, ink: [u8; 4]) {
        if value < 0 {
            self.push(MINUS, ink);
        }
        let magnitude = value.unsigned_abs().min(MOST);
        if magnitude >= 10 {
            self.push((magnitude / 10) as usize, ink);
        }
        self.push((magnitude % 10) as usize, ink);
    }

    fn pieces(&self) -> &[(usize, [u8; 4])] {
        &self.pieces[..self.len]
    }
}

/// One badge's shape on the card's face, in millimetres from the face's middle, x to the right
/// and y up.
struct Plate {
    /// Its middle.
    at: Vec2,
    size: Vec2,
    radius: f32,
    border: f32,
    fill: [u8; 4],
    edge: [u8; 4],
    /// The height of its numerals.
    numerals: f32,
}

/// Where a card's badges go: the card's pose, the band they are drawn in and their opacity.
pub(super) struct On {
    pub pose: Pose,
    pub band: Band,
    pub opacity: f32,
}

impl Badges {
    /// Writes the badges of `object`, a card shown with `face`, standing for `copies` identical
    /// permanents, onto the card `on` describes. Only a permanent on the battlefield wears
    /// them. A card writes no more than `BUDGET` records: a badge that would pass it is left
    /// out, the plaque first kept, then the pile's count, then the counters in order.
    pub(super) fn write(
        &self,
        (object, face, copies): (&ObjectView, Option<&ListedFace>, u16),
        on: &On,
        builder: &mut FrameBuilder,
    ) {
        if object.zone != ZoneName::Battlefield || self.pieces.is_empty() {
            return;
        }
        // The side that faces up, so that a face-down permanent wears its badges on its back
        // as the web interface draws them, and a card turning over turns them with it.
        let card = on.pose.upward();
        let mut left = BUDGET;
        let corner = Vec2::new(CARD_WIDTH, CARD_HEIGHT) / 2.0;
        if let Some((label, fill)) = plaque(object, face) {
            let height = data::plaque::HEIGHT * em();
            let numerals = data::plaque::NUMERALS * em();
            let width = (self.width(&label, numerals) + 2.0 * data::plaque::PADDING * em())
                .max(data::plaque::MIN_WIDTH * em());
            let right = corner.x - data::plaque::RIGHT * em();
            let bottom = -corner.y + data::plaque::BOTTOM * em();
            let plate = Plate {
                at: Vec2::new(right - width / 2.0, bottom + height / 2.0),
                size: Vec2::new(width, height),
                radius: data::plaque::RADIUS * em(),
                border: data::plaque::BORDER * em(),
                fill,
                edge: badge::PLAQUE_BORDER,
                numerals,
            };
            self.badge(&plate, &label, (card, on), &mut left, builder);
        }
        if copies > 1 {
            let mut label = Label::default();
            label.push(TIMES, badge::COUNT_INK);
            label.number(i32::from(copies), badge::COUNT_INK);
            let numerals = data::count::NUMERALS * em();
            let width = self.width(&label, numerals) + 2.0 * data::count::PADDING * em();
            let height = data::count::HEIGHT * em();
            let right = corner.x - data::count::RIGHT * em();
            let top = corner.y - data::count::TOP * em();
            let plate = Plate {
                at: Vec2::new(right - width / 2.0, top - height / 2.0),
                size: Vec2::new(width, height),
                radius: data::count::RADIUS * em(),
                border: data::count::BORDER * em(),
                fill: badge::COUNT,
                edge: badge::COUNT_BORDER,
                numerals,
            };
            self.badge(&plate, &label, (card, on), &mut left, builder);
        }
        // Along the foot, left to right, as far as the plaque.
        let mut x = -corner.x + data::counter::LEFT * em();
        let end = corner.x - data::counter::RIGHT * em();
        for (count, mark) in counters(object) {
            let mut label = Label::default();
            label.number(count, mark);
            let numerals = data::counter::NUMERALS * em();
            let width = self.width(&label, numerals) + 2.0 * data::counter::PADDING * em();
            if x + width > end {
                break;
            }
            let height = data::counter::HEIGHT * em();
            let bottom = -corner.y + data::counter::BOTTOM * em();
            let plate = Plate {
                at: Vec2::new(x + width / 2.0, bottom + height / 2.0),
                size: Vec2::new(width, height),
                radius: data::counter::RADIUS * em(),
                border: data::counter::BORDER * em(),
                fill: badge::COUNTER,
                edge: color::METAL_BRIGHT,
                numerals,
            };
            self.badge(&plate, &label, (card, on), &mut left, builder);
            x += width + data::counter::GAP * em();
        }
    }

    /// How wide `label` is with numerals `numerals` millimetres high.
    fn width(&self, label: &Label, numerals: f32) -> f32 {
        let points: f32 = label
            .pieces()
            .iter()
            .map(|&(piece, _)| self.pieces[piece].width)
            .sum();
        points * numerals / STYLE.size
    }

    /// Writes `plate` and `label` centred on it onto `card`, if their records fit in `left`.
    fn badge(
        &self,
        plate: &Plate,
        label: &Label,
        (card, on): (Affine3A, &On),
        left: &mut usize,
        builder: &mut FrameBuilder,
    ) {
        let records = 1 + label
            .pieces()
            .iter()
            .map(|&(piece, _)| self.pieces[piece].glyphs.len())
            .sum::<usize>();
        if records > *left {
            return;
        }
        *left -= records;
        let raise = data::RAISE * em();
        let quad = |at: Vec2, size: Vec2, height: f32| {
            card * Affine3A::from_scale_rotation_translation(
                size.extend(1.0),
                Quat::IDENTITY,
                at.extend(height),
            )
        };
        builder.draw(
            on.band,
            Draw::panel(
                quad(plate.at, plate.size, raise),
                faded(plate.fill, on.opacity),
                faded(plate.edge, on.opacity),
                plate.size,
                plate.radius,
                plate.border,
            ),
        );
        // Millimetres per point, and the text box's top left corner.
        let k = plate.numerals / STYLE.size;
        let width = self.width(label, plate.numerals);
        let mut pen = Vec2::new(plate.at.x - width / 2.0, plate.at.y + plate.numerals / 2.0);
        let scale = self.scale;
        for &(piece, ink) in label.pieces() {
            let block = &self.pieces[piece];
            for glyph in &block.glyphs {
                let Some(placed) = self.glyphs.get(glyph_key(STYLE, scale, glyph.glyph)) else {
                    continue;
                };
                let [_, _, w, h] = placed.rect;
                let size = Vec2::new(f32::from(w), f32::from(h)) / scale * k;
                let top_left = Vec2::new(
                    glyph.x + f32::from(placed.bearing[0]) / scale,
                    glyph.y + f32::from(placed.bearing[1]) / scale,
                );
                let at = pen
                    + Vec2::new(
                        top_left.x * k + size.x / 2.0,
                        -top_left.y * k - size.y / 2.0,
                    );
                builder.draw(
                    on.band,
                    Draw::glyph(
                        quad(at, size, 2.0 * raise),
                        faded(ink, on.opacity),
                        Sprite {
                            page: placed.page,
                            rect: placed.rect,
                        },
                    ),
                );
            }
            pen.x += block.width * k;
        }
    }
}

/// The plaque's label and fill: power and toughness for a creature, loyalty for a
/// planeswalker, or none.
fn plaque(object: &ObjectView, face: Option<&ListedFace>) -> Option<(Label, [u8; 4])> {
    let fill = match face.map_or(&[][..], |face| face.colors.as_slice()) {
        [] => badge::PLAQUE_COLORLESS,
        [only] => match only.as_str() {
            "W" => badge::PLAQUE_WHITE,
            "U" => badge::PLAQUE_BLUE,
            "B" => badge::PLAQUE_BLACK,
            "R" => badge::PLAQUE_RED,
            "G" => badge::PLAQUE_GREEN,
            _ => badge::PLAQUE_COLORLESS,
        },
        _ => badge::PLAQUE_GOLD,
    };
    let mut label = Label::default();
    if let Some(pt) = object.pt {
        let printed = face.and_then(|face| face.printed);
        let power = Tone::of(pt.power, printed.map(|[power, _]| power));
        let toughness = if object.damage > 0 {
            Tone::Down
        } else {
            Tone::of(pt.toughness, printed.map(|[_, toughness]| toughness))
        };
        label.number(pt.power, power.ink());
        label.push(SLASH, badge::PLAQUE_INK);
        label.number((pt.toughness - object.damage).max(0), toughness.ink());
        return Some((label, fill));
    }
    let loyalty = object
        .counters
        .iter()
        .find(|counter| counter.kind == Counter::Loyalty)?;
    label.number(loyalty.count, badge::PLAQUE_INK);
    Some((label, fill))
}

/// The count of each kind of counter on `object` but loyalty, which its plaque shows, and the
/// color its number is drawn in (`.mark-plusOne`, `.mark-minusOne` and `.mark-badge svg`'s
/// gold): the rules' kinds first, then the manual table's named ones.
fn counters(object: &ObjectView) -> impl Iterator<Item = (i32, [u8; 4])> + '_ {
    let kinds = object
        .counters
        .iter()
        .filter(|counter| counter.kind != Counter::Loyalty && counter.count != 0)
        .map(|counter| {
            let mark = match counter.kind {
                Counter::PlusOne => color::STAT_UP,
                Counter::MinusOne => color::STAT_DOWN,
                _ => color::GOLD,
            };
            (counter.count, mark)
        });
    let named = object
        .named_counters
        .iter()
        .filter(|counter| counter.count != 0)
        .map(|counter| (counter.count, color::GOLD));
    kinds.chain(named)
}

/// `color` with its alpha scaled by `opacity`.
fn faded(color: [u8; 4], opacity: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = color;
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}
