//! The lines of text a permanent wears on its face besides its badges' numbers, as the web
//! interface's `Card` (`ui/src/components/Card.tsx`) draws them: its manual table note, or the
//! value it chose as it entered, on one line near its top (`.card-note`), cut with an ellipsis
//! to the card's width; and, for a copy, a "Copy" label at the start of its counters' row
//! (`.token-label`).
//!
//! Unlike a badge's digits, a note can say anything, so each text is shaped when a view that
//! holds it arrives (`Tags::prepare`) and kept, already cut to the card's width, so that a
//! frame only places glyphs it already has.

use std::collections::BTreeMap;

use beryl_protocol::protocol::{ObjectView, PlayerView, ZoneName};
use glam::{Affine3A, Vec2};

use super::{Badges, On, Plate, panel};
use kit_base::data::BadgeTuning;
use kit_base::tokens::{badge, color};
use kit_frame::frame::FrameBuilder;
use kit_text::text::fonts::Font;
use kit_text::text::{Block, Glyphs, Text, TypeStyle};

/// The style a note or a chosen value is shaped in: the interface font's bold
/// (`.card-note`'s `font`), at `note.points`.
fn note_style(tuning: &BadgeTuning) -> TypeStyle {
    TypeStyle {
        font: Font::OpenSans,
        weight: 700,
        italic: false,
        size: tuning.note.points,
        line_height: 1.0,
        letter_spacing: 0.0,
    }
}

/// The style of a copy's label (`.token-label`'s `font`), at `copy.points`.
fn copy_style(tuning: &BadgeTuning) -> TypeStyle {
    TypeStyle {
        size: tuning.copy.points,
        ..note_style(tuning)
    }
}

/// A line of text, shaped and cut to the room it has on a card.
#[derive(Debug)]
pub(super) struct Line {
    pub block: Block,
    /// How many of `block`'s glyphs are drawn.
    pub kept: usize,
    /// Where the ellipsis starts, in points from the line's left, when the line was cut.
    pub ellipsis: Option<f32>,
}

/// The shaped lines of the views so far.
#[derive(Debug, Default)]
pub(super) struct Tags {
    /// By the text a permanent says (`said`): its line, cut to a card's width.
    lines: BTreeMap<String, Line>,
    /// "Copy", once some view holds a copy.
    copy: Option<Line>,
    /// The ellipsis a cut line ends with, once some line is cut.
    ellipsis: Option<Block>,
}

impl Tags {
    /// Shapes the line of each permanent of `view` that says something and was not shaped
    /// before, and the copy label if `view` holds the first copy, and asks `wanted` for their
    /// glyphs at `scale`, at the sizes `tuning` sets.
    pub fn prepare(
        &mut self,
        view: &PlayerView,
        text: &mut Text,
        wanted: &mut Glyphs,
        (scale, tuning): (f32, &BadgeTuning),
    ) {
        let (note_style, room) = (note_style(tuning), note_room(tuning));
        let permanents =
            (view.objects.iter()).filter(|object| object.zone == ZoneName::Battlefield);
        for object in permanents {
            if let Some(said) = said(object)
                && !self.lines.contains_key(said)
            {
                // A line break would start a second line over the card's art.
                let block = text.block(&said.replace('\n', " "), note_style, &[], None);
                let mut line = whole(block.clone());
                if block.width > room {
                    let ellipsis = self.ellipsis.get_or_insert_with(|| {
                        let ellipsis = text.block("\u{2026}", note_style, &[], None).clone();
                        wanted.want(&ellipsis, scale);
                        ellipsis
                    });
                    line = cut(line.block, room - ellipsis.width);
                }
                wanted.want(&line.block, scale);
                self.lines.insert(said.to_owned(), line);
            }
            if object.really.is_some() && self.copy.is_none() {
                let block = text.block("Copy", copy_style(tuning), &[], None).clone();
                wanted.want(&block, scale);
                self.copy = Some(whole(block));
            }
        }
    }

    /// Asks `wanted` for every shaped line's glyphs at `scale`.
    pub fn want(&self, wanted: &mut Glyphs, scale: f32) {
        let lines = (self.lines.values())
            .chain(&self.copy)
            .map(|line| &line.block);
        for block in lines.chain(&self.ellipsis) {
            wanted.want(block, scale);
        }
    }

    /// What `object` says and its shaped line, if it says something and the line is shaped.
    pub fn note(&self, object: &ObjectView) -> Option<(&str, &Line)> {
        let (said, line) = self.lines.get_key_value(said(object)?)?;
        Some((said.as_str(), line))
    }

    /// The copy label's line, if `object` is a copy and the label is shaped.
    pub fn copy(&self, object: &ObjectView) -> Option<&Line> {
        object.really.as_ref().and(self.copy.as_ref())
    }

    /// The ellipsis a cut line ends with.
    pub fn ellipsis(&self) -> Option<&Block> {
        self.ellipsis.as_ref()
    }
}

impl Badges {
    /// Writes a copy's label, `line`, onto `card` with its left edge `x` millimetres from the
    /// face's middle, on the card's foot, if the frame has room for its
    /// records. Returns how wide it is, or zero when it was left out.
    pub(super) fn copy_label(
        &self,
        line: &Line,
        x: f32,
        (card, on): (Affine3A, &On),
        builder: &mut FrameBuilder,
    ) -> f32 {
        if !builder.take_text_room(1 + line.kept) {
            return 0.0;
        }
        let text = self.tuning().copy.text * self.em();
        let k = text / self.tuning().copy.points;
        let border = self.tuning().copy.border * self.em();
        let size = Vec2::new(
            line.block.width * k + 2.0 * (self.tuning().copy.padding_x * self.em() + border),
            text * self.tuning().copy.line_height
                + 2.0 * (self.tuning().copy.padding_y * self.em() + border),
        );
        let middle = -self.sizes().card.height / 2.0
            + (self.tuning().counter.bottom + self.tuning().counter.height / 2.0) * self.em();
        let plate = Plate {
            at: Vec2::new(x + size.x / 2.0, middle),
            size,
            radius: self.tuning().copy.radius * self.em(),
            border,
            fill: badge::COPY,
            edge: badge::COPY_BORDER,
            numerals: text,
        };
        panel(&plate, (card, on, self.raise()), builder);
        let pen = Vec2::new(
            x + border + self.tuning().copy.padding_x * self.em(),
            middle + text / 2.0,
        );
        let glyphs = &line.block.glyphs[..line.kept];
        self.glyphs(
            copy_style(self.tuning()),
            glyphs,
            (pen, k, badge::COPY_INK),
            (card, on),
            builder,
        );
        size.x
    }

    /// Writes a permanent's note or chosen value, `line`, onto `card` on a panel across its
    /// top, if the frame has room for its records, and says whether it did.
    pub(super) fn note(
        &self,
        line: &Line,
        (card, on): (Affine3A, &On),
        builder: &mut FrameBuilder,
    ) -> bool {
        let ellipsis = line.ellipsis.zip(self.tags.ellipsis());
        let records = 1 + line.kept + ellipsis.map_or(0, |(_, block)| block.glyphs.len());
        if !builder.take_text_room(records) {
            return false;
        }
        let text = self.tuning().note.text * self.em();
        let k = text / self.tuning().note.points;
        let size = Vec2::new(
            self.sizes().card.width - 2.0 * self.tuning().note.side * self.em(),
            text * self.tuning().note.line_height + 2.0 * self.tuning().note.padding_y * self.em(),
        );
        let top = self.sizes().card.height / 2.0 - self.tuning().note.top * self.em();
        let plate = Plate {
            at: Vec2::new(0.0, top - size.y / 2.0),
            size,
            radius: self.tuning().note.radius * self.em(),
            border: 0.0,
            fill: badge::NOTE,
            edge: badge::NOTE,
            numerals: text,
        };
        panel(&plate, (card, on, self.raise()), builder);
        let left = -size.x / 2.0 + self.tuning().note.padding_x * self.em();
        let pen = Vec2::new(left, plate.at.y + text / 2.0);
        let ink = (pen, k, color::TYPE);
        let glyphs = &line.block.glyphs[..line.kept];
        self.glyphs(note_style(self.tuning()), glyphs, ink, (card, on), builder);
        if let Some((at, block)) = ellipsis {
            let pen = (pen + Vec2::new(at * k, 0.0), k, color::TYPE);
            self.glyphs(
                note_style(self.tuning()),
                &block.glyphs,
                pen,
                (card, on),
                builder,
            );
        }
        true
    }
}

/// The one line a permanent shows near its top: its manual table note, or else the value it
/// chose as it entered, as the web's `Look` passes `object.note ?? object.chosen`, or a short
/// tag when a text-changing effect has altered its text or type line.
fn said(object: &ObjectView) -> Option<&str> {
    (object.note.as_deref())
        .or(object.chosen.as_deref())
        .filter(|said| !said.is_empty())
        .or_else(|| (object.text.is_some() || object.type_line.is_some()).then_some("Text changed"))
}

/// The room a note's line has, in points: the card's width less the panel's margins and its
/// padding.
fn note_room(tuning: &BadgeTuning) -> f32 {
    let note = &tuning.note;
    (1.0 / tuning.em - 2.0 * note.side - 2.0 * note.padding_x) * note.points / note.text
}

/// `block` drawn whole.
fn whole(block: Block) -> Line {
    Line {
        kept: block.glyphs.len(),
        block,
        ellipsis: None,
    }
}

/// `block` cut to the glyphs that end within `room` points, with the ellipsis after them.
fn cut(block: Block, room: f32) -> Line {
    let end = |at: usize| block.glyphs.get(at + 1).map_or(block.width, |next| next.x);
    let kept = (0..block.glyphs.len())
        .take_while(|&at| end(at) <= room)
        .count();
    let ellipsis = kept.checked_sub(1).map_or(0.0, end);
    Line {
        block,
        kept,
        ellipsis: Some(ellipsis),
    }
}

#[cfg(test)]
mod tests {
    use beryl_protocol::protocol::{ObjectId, ObjectView, Side};

    use super::*;
    use kit_text::text::Text;

    fn permanent(note: Option<&str>, chosen: Option<&str>) -> ObjectView {
        let mut object = ObjectView::neutral(ObjectId(1), ZoneName::Battlefield, Side::You);
        object.name = Some("Painter's Servant".to_owned());

        object.note = note.map(str::to_owned);
        object.chosen = chosen.map(str::to_owned);
        object
    }

    #[test]
    fn a_note_wins_over_a_chosen_value() {
        assert_eq!(said(&permanent(Some("Mine"), Some("Red"))), Some("Mine"));
        assert_eq!(said(&permanent(None, Some("Red"))), Some("Red"));
        assert_eq!(said(&permanent(Some(""), None)), None);
    }

    #[test]
    fn a_long_note_is_cut_to_the_card_with_an_ellipsis_and_a_short_one_is_not() {
        let tuning =
            BadgeTuning::parse(include_str!("../../../../../../data/client/badges.json")).unwrap();
        let (note_style, room) = (note_style(&tuning), note_room(&tuning));
        let mut text = Text::new();
        let short = text.block("Red", note_style, &[], None).clone();
        let long = (text.block(
            &"Exiled until this leaves ".repeat(3),
            note_style,
            &[],
            None,
        ))
        .clone();
        let ellipsis = text.block("\u{2026}", note_style, &[], None).width;
        assert!(short.width <= room);
        let line = cut(long, room - ellipsis);
        assert!(0 < line.kept && line.kept < line.block.glyphs.len());
        let at = line.ellipsis.expect("a cut line ends with an ellipsis");
        assert!(at + ellipsis <= room && at > room / 2.0);
    }
}
