//! What a permanent's badges say: the plaque's power and toughness or loyalty, inked against
//! the printed numbers, and the count of each kind of counter, as pieces of shaped text.

use beryl_protocol::protocol::{Counter, ObjectView};

use super::{LONGEST, MINUS, MOST, SLASH};
use crate::faces::ListedFace;
use kit_base::tokens::{badge, color};

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
pub(super) struct Label {
    pieces: [(usize, [u8; 4]); LONGEST],
    len: usize,
}

impl Label {
    pub(super) fn push(&mut self, piece: usize, ink: [u8; 4]) {
        if self.len < LONGEST {
            self.pieces[self.len] = (piece, ink);
            self.len += 1;
        }
    }

    /// `value` as pieces, with a minus sign when it is below zero.
    pub(super) fn number(&mut self, value: i32, ink: [u8; 4]) {
        if value < 0 {
            self.push(MINUS, ink);
        }
        let magnitude = value.unsigned_abs().min(MOST);
        if magnitude >= 10 {
            self.push((magnitude / 10) as usize, ink);
        }
        self.push((magnitude % 10) as usize, ink);
    }

    pub(super) fn pieces(&self) -> &[(usize, [u8; 4])] {
        &self.pieces[..self.len]
    }
}

/// The plaque's label and fill: power and toughness for a creature, loyalty for a
/// planeswalker, or none.
pub(super) fn plaque(object: &ObjectView, face: Option<&ListedFace>) -> Option<(Label, [u8; 4])> {
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
/// color its number is drawn in: the bright stat colors for +1/+1 and -1/-1 counters, and the
/// disc's pale ink for every other kind. The manual table's named counters have a row of their
/// own (`write_named`).
pub(super) fn counters(object: &ObjectView) -> impl Iterator<Item = (i32, [u8; 4])> + '_ {
    object
        .counters
        .iter()
        .filter(|counter| counter.kind != Counter::Loyalty && counter.count != 0)
        .map(|counter| {
            let mark = match counter.kind {
                Counter::PlusOne => color::STAT_UP,
                Counter::MinusOne => color::STAT_DOWN,
                _ => badge::COUNTER_DISC_INK,
            };
            (counter.count, mark)
        })
}
