//! The badges a permanent wears on its face (docs/beryl-3d.md, "Objects and placement").
//! A permanent wears its power and toughness on a plaque at the bottom right, or its loyalty
//! there for a planeswalker; a badge for each kind of counter on it, along its foot; and, on the top card of
//! a pile of identical permanents, how many the pile holds; on the manual table, a lock in its
//! top left corner if it doesn't untap, and a badge for each named counter, with the counter's
//! shortened name, in a row above the plaque. They follow the web interface's
//! `Card` (`ui/src/components/Card.tsx`): a plaque's numbers are inked up or down against the
//! printed ones (`toneClass`), and the toughness shown is less the damage marked on the
//! creature, inked down, as `frameFace` in `ui/src/game/faces.ts` draws it.
//!
//! Each badge is a rounded panel lying on the card's face, which moves and turns with the card,
//! and its number is glyph records. Its sizes are the web's, in the em of a battlefield card
//! (a tenth of its width), from `data/client/badges.json`. The digits and signs are
//! shaped when the first view arrives and again when that data changes (`Badges::shape`), so a frame only places glyphs it
//! already has and allocates nothing. A named counter's name is shaped when the first view that
//! names it arrives (`Badges::shape_names`), and the lock is the flat layer's `LOCK` icon. The
//! lines of text a permanent wears besides, its note or chosen value and a copy's label, are
//! `tags`.

mod labels;
mod tags;

use beryl_protocol::protocol::{ObjectView, PlayerView, ZoneName};
use glam::{Affine3A, Quat, Vec2};
use labels::{Label, counters, plaque};

use crate::faces::ListedFace;
use kit_base::data::{BadgeTuning, Card, Data};
use kit_base::pose::Pose;
use kit_base::tokens::{badge, color};
use kit_frame::frame::{Band, Draw, FrameBuilder, RECORDS_PER_OBJECT, Sprite};
use kit_text::text::fonts::Font;
use kit_text::text::{Block, Glyphs, PlacedGlyph, Text, TypeStyle, glyph_key};
use kit_ui::input::{GlyphCell, IconCell};
use kit_ui::widgets::icons::{LOCK, LOCK_SIDE};

/// The style every badge's number is shaped in: the card font's bold, at the size in points a
/// plaque's numerals have on a battlefield card of the web interface (`numeral_points`).
#[must_use]
pub fn style(tuning: &BadgeTuning) -> TypeStyle {
    TypeStyle {
        font: Font::CrimsonPro,
        weight: 700,
        italic: false,
        size: tuning.numeral_points,
        line_height: 1.0,
        letter_spacing: 0.0,
    }
}

/// The style a named counter's name is shaped in: the interface font's bold, which the web's
/// `.mark-badge` sets its label in, at the size the numerals are shaped at, so that both are
/// drawn from cells of about the same size.
#[must_use]
pub fn name_style(tuning: &BadgeTuning) -> TypeStyle {
    TypeStyle {
        font: Font::OpenSans,
        ..style(tuning)
    }
}

/// The sizes badges are shaped and drawn at: the badges' tuning and the card they lie on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BadgeSizes {
    pub tuning: BadgeTuning,
    pub card: Card,
}

impl BadgeSizes {
    /// The sizes `data` sets.
    #[must_use]
    pub fn of(data: &Data) -> Self {
        Self {
            tuning: data.badges,
            card: data.card,
        }
    }

    /// One millimetre of the web card's em, a share of a battlefield card's width.
    fn em(&self) -> f32 {
        self.tuning.em * self.card.width
    }
}

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

/// The records a permanent's badges may write: every record a card may write besides itself,
/// its soft edge, its contact shadow and a mark (`RECORDS_PER_OBJECT`). That is a plaque of
/// single-digit power and toughness (a panel and three glyphs), the manual table's lock (a
/// panel and its icon) and a named counter of six letters and a single-digit count (a panel and
/// seven glyphs), or the plaque, the lock and four single-digit counter badges.
const BUDGET: usize = RECORDS_PER_OBJECT - 4;
/// The most pieces of text one badge holds: `-12/-34`.
const LONGEST: usize = 7;

/// The shaped numbers and signs badges are made of, and where the glyph atlas holds their
/// glyphs.
#[derive(Debug, Default)]
pub struct Badges {
    /// The sizes everything below was shaped for. None until shaped.
    sizes: Option<BadgeSizes>,
    /// By piece: the digits, then `+`, `-`, `/` and `×`. Empty until shaped.
    pieces: Vec<Block>,
    glyphs: Glyphs,
    /// Physical pixels per logical point, which picks the glyphs' size in the atlas.
    scale: f32,
    /// The shaped label of each named counter a view has named, by the counter's whole name,
    /// sorted by it: the name itself, or its first five letters and an ellipsis if it is longer
    /// than six, as the web's `NamedCounterBadge` cuts it.
    names: Vec<(Box<str>, Block)>,
    /// Where the atlas holds the lock, at the latest scale it arrived at.
    lock: Option<Sprite>,
    /// The lines of text of the permanents of the views so far.
    tags: tags::Tags,
}

impl Badges {
    /// Whether the pieces are shaped.
    #[must_use]
    pub fn shaped(&self) -> bool {
        !self.pieces.is_empty()
    }

    /// Whether the pieces are shaped at `sizes`.
    #[must_use]
    pub fn shaped_at(&self, sizes: &BadgeSizes) -> bool {
        self.shaped() && self.sizes.as_ref() == Some(sizes)
    }

    /// Shapes every piece with `text` at `sizes`, and asks `wanted` for their glyphs at `scale`.
    /// New sizes discard shaped names and lines of text. The callers shape them again
    /// through `shape_names` and `prepare` as views arrive.
    pub fn shape(&mut self, text: &mut Text, wanted: &mut Glyphs, scale: f32, sizes: BadgeSizes) {
        self.scale = scale;
        if !self.shaped_at(&sizes) {
            self.sizes = Some(sizes);
            self.names.clear();
            self.tags = tags::Tags::default();
            let pieces = [
                "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "+", "-", "/", "\u{d7}",
            ];
            let style = style(&sizes.tuning);
            self.pieces = pieces
                .into_iter()
                .map(|piece| text.block(piece, style, &[], None).clone())
                .collect();
            debug_assert_eq!(self.pieces.len(), PIECES);
        }
        self.want(wanted);
    }

    /// The sizes the badges were shaped at.
    ///
    /// # Panics
    ///
    /// If they are not shaped. Every caller checks first, since nothing is drawn before then.
    fn sizes(&self) -> &BadgeSizes {
        self.sizes
            .as_ref()
            .expect("the badges are shaped before they are drawn")
    }

    fn tuning(&self) -> &BadgeTuning {
        &self.sizes().tuning
    }

    /// One millimetre of the web card's em, a share of a battlefield card's width.
    fn em(&self) -> f32 {
        self.sizes().em()
    }

    /// How far a badge's panel lies above the card's face, in millimetres.
    fn raise(&self) -> f32 {
        self.tuning().raise * self.em()
    }

    fn style(&self) -> TypeStyle {
        style(self.tuning())
    }

    fn name_style(&self) -> TypeStyle {
        name_style(self.tuning())
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

    /// Records where the lock is in an atlas page of icons that is ready. The pages of a new
    /// display scale arrive after the old one's, so the latest cell is at the current scale.
    pub fn add_icons(&mut self, page: u16, cells: &[IconCell]) {
        if let Some(cell) = cells
            .iter()
            .rfind(|cell| cell.icon == LOCK && cell.size == LOCK_SIDE && cell.shadow == 0)
        {
            self.lock = Some(Sprite {
                page,
                rect: cell.rect,
            });
        }
    }

    /// Shapes the label of every named counter on a permanent of `objects` not shaped yet, and
    /// asks `wanted` for its glyphs. A name already shaped costs a look-up.
    pub fn shape_names<'a>(
        &mut self,
        text: &mut Text,
        wanted: &mut Glyphs,
        objects: impl IntoIterator<Item = &'a ObjectView>,
    ) {
        if self.sizes.is_none() {
            return;
        }
        let names = objects
            .into_iter()
            .flat_map(|object| object.named_counters.iter());
        for counter in names {
            let Err(at) = self
                .names
                .binary_search_by(|(shaped, _)| (**shaped).cmp(&counter.name))
            else {
                continue;
            };
            let block = text
                .block(&counter_label(&counter.name), self.name_style(), &[], None)
                .clone();
            wanted.want(&block, self.scale);
            self.names.insert(at, (counter.name.as_str().into(), block));
        }
    }

    /// Shapes the line of text of each permanent in `view` that wears one (`tags`), once for
    /// each text, and asks `wanted` for their glyphs. Called for every view as it arrives,
    /// after `shape`, so that a frame shapes none.
    pub fn prepare(&mut self, view: &PlayerView, text: &mut Text, wanted: &mut Glyphs) {
        let Some(sizes) = self.sizes else {
            return;
        };
        self.tags
            .prepare(view, text, wanted, (self.scale, &sizes.tuning));
    }

    /// The shaped label of the named counter called `name`, if it is shaped.
    fn name(&self, name: &str) -> Option<&Block> {
        let at = self
            .names
            .binary_search_by(|(shaped, _)| (**shaped).cmp(name))
            .ok()?;
        Some(&self.names[at].1)
    }

    fn want(&self, wanted: &mut Glyphs) {
        for block in &self.pieces {
            wanted.want(block, self.scale);
        }
        for (_, block) in &self.names {
            wanted.want(block, self.scale);
        }
        self.tags.want(wanted, self.scale);
    }
}

/// `name` as a named counter's badge shows it: whole up to six letters, and otherwise its first
/// five and an ellipsis (`NamedCounterBadge` in `ui/src/components/Card.tsx`).
#[must_use]
pub fn counter_label(name: &str) -> String {
    if name.chars().count() > 6 {
        name.chars().take(5).chain(['\u{2026}']).collect()
    } else {
        name.to_owned()
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
    /// The height of its numerals or text.
    numerals: f32,
}

/// A named counter's label before its count: its shaped name, drawn `size` millimetres high
/// in `ink`, and the gap after it.
struct Name<'a> {
    block: &'a Block,
    /// The style the name was shaped in.
    style: TypeStyle,
    size: f32,
    gap: f32,
    ink: [u8; 4],
}

impl Name<'_> {
    /// Its width with the gap, in millimetres.
    fn width(&self) -> f32 {
        self.block.width * self.size / self.style.size + self.gap
    }
}

/// Where a card's badges go: the card's pose, the band they are drawn in and their opacity,
/// and whether a group of identical cards shows its copy label.
#[derive(Clone, Copy)]
pub(super) struct On {
    pub pose: Pose,
    pub band: Band,
    pub opacity: f32,
    pub show_copy_label: bool,
}

impl Badges {
    /// Writes the badges of `object`, a card shown with `face`, standing for `copies` identical
    /// permanents, onto the card `on` describes. Only a permanent on the battlefield wears
    /// them. A card writes no more than `BUDGET` records for its badges: a badge that would
    /// pass it is left out, the plaque first kept, then the pile's count, then the lock, then
    /// the counters in order, then the named counters in order. Its lines of text, its copy
    /// label and its note, take their records from the frame's `TEXT_RECORDS` instead. Returns
    /// the text of the note line it wrote.
    pub(super) fn write(
        &self,
        (object, face, copies): (&ObjectView, Option<&ListedFace>, u16),
        on: &On,
        builder: &mut FrameBuilder,
    ) -> Option<&str> {
        if object.zone != ZoneName::Battlefield || self.pieces.is_empty() {
            return None;
        }
        // The side that faces up, so that a face-down permanent wears its badges on its back
        // as the web interface draws them, and a card turning over turns them with it.
        let card = on.pose.upward();
        let mut left = BUDGET;
        let corner = self.sizes().card.half();
        // Copies beneath the top card peek out of the pile but share its plaque.
        if copies > 0
            && let Some((label, fill)) = plaque(object, face)
        {
            let height = self.tuning().plaque.height * self.em();
            let numerals = self.tuning().plaque.numerals * self.em();
            let width = (self.width(&label, numerals)
                + 2.0 * self.tuning().plaque.padding * self.em())
            .max(self.tuning().plaque.min_width * self.em());
            let right = corner.x - self.tuning().plaque.right * self.em();
            let bottom = -corner.y + self.tuning().plaque.bottom * self.em();
            let plate = Plate {
                at: Vec2::new(right - width / 2.0, bottom + height / 2.0),
                size: Vec2::new(width, height),
                radius: self.tuning().plaque.radius * self.em(),
                border: self.tuning().plaque.border * self.em(),
                fill,
                edge: badge::PLAQUE_BORDER,
                numerals,
            };
            let _ = self.badge(&plate, (None, &label), (card, on), &mut left, builder);
        }
        if copies > 1 {
            let mut label = Label::default();
            label.push(TIMES, badge::COUNT_INK);
            label.number(i32::from(copies), badge::COUNT_INK);
            let numerals = self.tuning().count.numerals * self.em();
            let width =
                self.width(&label, numerals) + 2.0 * self.tuning().count.padding * self.em();
            let height = self.tuning().count.height * self.em();
            let right = corner.x - self.tuning().count.right * self.em();
            let top = corner.y - self.tuning().count.top * self.em();
            let plate = Plate {
                at: Vec2::new(right - width / 2.0, top - height / 2.0),
                size: Vec2::new(width, height),
                radius: self.tuning().count.radius * self.em(),
                border: self.tuning().count.border * self.em(),
                fill: badge::COUNT,
                edge: badge::COUNT_BORDER,
                numerals,
            };
            let _ = self.badge(&plate, (None, &label), (card, on), &mut left, builder);
        }
        if object.doesnt_untap {
            self.write_lock(corner, (card, on), &mut left, builder);
        }
        // Along the foot, left to right, as far as the plaque: a copy's label, then the
        // counters.
        let mut x = -corner.x + self.tuning().counter.left * self.em();
        let end = corner.x - self.tuning().counter.right * self.em();
        if let Some(line) = self.tags.copy(object).filter(|_| on.show_copy_label) {
            let width = self.copy_label(line, x, (card, on), builder);
            if width > 0.0 {
                x += width + self.tuning().counter.gap * self.em();
            }
        }
        for (count, mark) in counters(object) {
            let mut label = Label::default();
            label.number(count, mark);
            let numerals = self.tuning().counter.numerals * self.em();
            let width =
                self.width(&label, numerals) + 2.0 * self.tuning().counter.padding * self.em();
            if x + width > end {
                break;
            }
            let height = self.tuning().counter.height * self.em();
            let bottom = -corner.y + self.tuning().counter.bottom * self.em();
            let plate = Plate {
                at: Vec2::new(x + width / 2.0, bottom + height / 2.0),
                size: Vec2::new(width, height),
                radius: self.tuning().counter.radius * self.em(),
                border: self.tuning().counter.border * self.em(),
                fill: badge::COUNTER,
                edge: color::METAL_BRIGHT,
                numerals,
            };
            let _ = self.badge(&plate, (None, &label), (card, on), &mut left, builder);
            x += width + self.tuning().counter.gap * self.em();
        }
        self.write_named(object, corner, (card, on), &mut left, builder);
        let (said, line) = self.tags.note(object)?;
        self.note(line, (card, on), builder).then_some(said)
    }

    /// Writes a badge for each named counter on `object` onto `card`, left to right in rows
    /// above the plaque, each its shortened name and then its count, as the web's
    /// `.card-named-counters` stands them: a badge that would pass the card's right edge
    /// starts a row above (`flex-wrap: wrap-reverse`). A counter whose name is not shaped yet,
    /// too wide for a row of its own or past the card's records, is left out with the ones
    /// after it, so that no later badge stands where it would have.
    fn write_named(
        &self,
        object: &ObjectView,
        corner: Vec2,
        (card, on): (Affine3A, &On),
        left: &mut usize,
        builder: &mut FrameBuilder,
    ) {
        let mut x = -corner.x + self.tuning().counter.left * self.em();
        let end = corner.x - self.tuning().counter.named_right * self.em();
        let numerals = self.tuning().counter.numerals * self.em();
        let height = self.tuning().counter.height * self.em();
        let start = x;
        let mut bottom = -corner.y + self.tuning().counter.named_bottom * self.em();
        for counter in object
            .named_counters
            .iter()
            .filter(|counter| counter.count != 0)
        {
            let Some(block) = self.name(&counter.name) else {
                break;
            };
            let name = Name {
                block,
                style: self.name_style(),
                size: self.tuning().counter.name * self.em(),
                gap: self.tuning().counter.name_gap * self.em(),
                ink: color::TYPE,
            };
            let mut label = Label::default();
            label.number(counter.count, color::TYPE);
            let width = name.width()
                + self.width(&label, numerals)
                + 2.0 * self.tuning().counter.padding * self.em();
            if start + width > end {
                break;
            }
            if x + width > end {
                x = start;
                bottom += height + self.tuning().counter.gap * self.em();
            }
            let plate = Plate {
                at: Vec2::new(x + width / 2.0, bottom + height / 2.0),
                size: Vec2::new(width, height),
                radius: self.tuning().counter.radius * self.em(),
                border: self.tuning().counter.border * self.em(),
                fill: badge::COUNTER,
                edge: color::METAL_BRIGHT,
                numerals,
            };
            if !self.badge(&plate, (Some(&name), &label), (card, on), left, builder) {
                break;
            }
            x += width + self.tuning().counter.gap * self.em();
        }
    }

    /// Writes the lock of a permanent that doesn't untap onto `card`, in its top left corner,
    /// as the web's `.card-untap-lock` stands a `MarkBadge` with the lock icon: a counter's
    /// panel with the icon in gold. Nothing is written before the lock's icon arrives.
    fn write_lock(
        &self,
        corner: Vec2,
        (card, on): (Affine3A, &On),
        left: &mut usize,
        builder: &mut FrameBuilder,
    ) {
        let Some(sprite) = self.lock else {
            return;
        };
        let lock = self.tuning().lock;
        if *left < 2 {
            return;
        }
        *left -= 2;
        let icon = lock.icon * self.em();
        let border = lock.border * self.em();
        let size = Vec2::new(
            icon + (lock.padding_left + lock.padding_right) * self.em() + 2.0 * border,
            lock.height * self.em(),
        );
        let top_left = Vec2::new(
            -corner.x + lock.left * self.em(),
            corner.y - lock.top * self.em(),
        );
        let at = top_left + Vec2::new(size.x, -size.y) / 2.0;
        let raise = self.raise();
        builder.draw(
            on.band,
            Draw::panel(
                quad(card, at, size, raise),
                faded(badge::COUNTER, on.opacity),
                faded(color::METAL_BRIGHT, on.opacity),
                size,
                lock.radius * self.em(),
                border,
            ),
        );
        let middle = Vec2::new(
            top_left.x + border + lock.padding_left * self.em() + icon / 2.0,
            at.y,
        );
        builder.draw(
            on.band,
            Draw::icon(
                quad(card, middle, Vec2::splat(icon), 2.0 * raise),
                faded(color::GOLD, on.opacity),
                sprite,
            ),
        );
    }

    /// How wide `label` is with numerals `numerals` millimetres high.
    fn width(&self, label: &Label, numerals: f32) -> f32 {
        let points: f32 = label
            .pieces()
            .iter()
            .map(|&(piece, _)| self.pieces[piece].width)
            .sum();
        points * numerals / self.style().size
    }

    /// Writes `plate` and, centred on it, `name` if any and then `label`, onto `card`, if
    /// their records fit in `left`. Returns whether they did.
    fn badge(
        &self,
        plate: &Plate,
        (name, label): (Option<&Name>, &Label),
        (card, on): (Affine3A, &On),
        left: &mut usize,
        builder: &mut FrameBuilder,
    ) -> bool {
        let records = 1
            + name.map_or(0, |name| name.block.glyphs.len())
            + label
                .pieces()
                .iter()
                .map(|&(piece, _)| self.pieces[piece].glyphs.len())
                .sum::<usize>();
        if records > *left {
            return false;
        }
        *left -= records;
        panel(plate, (card, on, self.raise()), builder);
        let width = name.map_or(0.0, Name::width) + self.width(label, plate.numerals);
        let mut x = plate.at.x - width / 2.0;
        if let Some(name) = name {
            let pen = Vec2::new(x, plate.at.y + name.size / 2.0);
            let glyphs = (name.block, name.style, name.size / name.style.size);
            self.text(glyphs, pen, name.ink, (card, on), builder);
            x += name.width();
        }
        // Millimetres per point, and the text box's top left corner.
        let k = plate.numerals / self.style().size;
        let mut pen = Vec2::new(x, plate.at.y + plate.numerals / 2.0);
        for &(piece, ink) in label.pieces() {
            let block = &self.pieces[piece];
            self.text((block, self.style(), k), pen, ink, (card, on), builder);
            pen.x += block.width * k;
        }
        true
    }

    /// Writes the glyphs of `block`, shaped in `style` and drawn at `k` millimetres a point,
    /// with the top left corner of its text box at `pen`, in `ink`, onto `card`.
    fn text(
        &self,
        (block, style, k): (&Block, TypeStyle, f32),
        pen: Vec2,
        ink: [u8; 4],
        (card, on): (Affine3A, &On),
        builder: &mut FrameBuilder,
    ) {
        self.glyphs(style, &block.glyphs, (pen, k, ink), (card, on), builder);
    }

    /// Writes `glyphs`, shaped in `style`, onto `card` in `ink`, with their text box's top
    /// left corner at `pen` on the card's face and `k` millimetres to the point.
    fn glyphs(
        &self,
        style: TypeStyle,
        glyphs: &[PlacedGlyph],
        (pen, k, ink): (Vec2, f32, [u8; 4]),
        (card, on): (Affine3A, &On),
        builder: &mut FrameBuilder,
    ) {
        let scale = self.scale;
        let raise = self.raise();
        for glyph in glyphs {
            let Some(placed) = self.glyphs.get(glyph_key(style, scale, glyph.glyph)) else {
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
                    quad(card, at, size, 2.0 * raise),
                    faded(ink, on.opacity),
                    Sprite {
                        page: placed.page,
                        rect: placed.rect,
                    },
                ),
            );
        }
    }
}

/// The transform of a quad of `size` millimetres lying `height` above the face of `card`, its
/// middle at `at` on the face.
fn quad(card: Affine3A, at: Vec2, size: Vec2, height: f32) -> Affine3A {
    card * Affine3A::from_scale_rotation_translation(
        size.extend(1.0),
        Quat::IDENTITY,
        at.extend(height),
    )
}

/// Writes `plate`'s rounded panel onto `card`, `raise` millimetres above its face.
fn panel(plate: &Plate, (card, on, raise): (Affine3A, &On, f32), builder: &mut FrameBuilder) {
    builder.draw(
        on.band,
        Draw::panel(
            quad(card, plate.at, plate.size, raise),
            faded(plate.fill, on.opacity),
            faded(plate.edge, on.opacity),
            plate.size,
            plate.radius,
            plate.border,
        ),
    );
}

/// `color` with its alpha scaled by `opacity`.
fn faded(color: [u8; 4], opacity: f32) -> [u8; 4] {
    let [red, green, blue, alpha] = color;
    let alpha = (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8;
    [red, green, blue, alpha]
}
