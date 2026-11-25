//! Where a permanent's counter discs stand: one for each kind of counter, top to bottom down
//! the card's right edge, as Arena stands a counter on a permanent. The first stands just
//! below the name bar, or below the pile's count when the card stands for more than one
//! permanent. Each passes the card's right edge by the tuning's `overhang`, and none reaches
//! the row the power and toughness plaque sits in.

use glam::Vec2;
use kit_base::data::BadgeTuning;

/// The column of discs on one card, in millimetres from the middle of its face, as the discs are
/// placed in turn.
#[derive(Clone, Copy, Debug)]
pub struct Column {
    /// The x of every disc's right edge.
    right: f32,
    /// The y of the next disc's top edge.
    top: f32,
    /// The lowest y a disc's bottom edge may reach: the plaque's top, less a gap.
    floor: f32,
    /// A disc's height, and its least width.
    size: f32,
    gap: f32,
}

impl Column {
    /// The column on a card whose face's top right corner is `corner`, with one millimetre of
    /// the badges' em being `em`, standing for `copies` permanents.
    #[must_use]
    pub fn new(tuning: &BadgeTuning, em: f32, corner: Vec2, copies: u16) -> Self {
        let disc = tuning.disc;
        let top = if copies > 1 {
            corner.y - (tuning.count.top + tuning.count.height + disc.gap) * em
        } else {
            corner.y - disc.top * em
        };
        let plaque = tuning.plaque;
        Self {
            right: corner.x + disc.overhang * em,
            top,
            floor: -corner.y + (plaque.bottom + plaque.height + disc.gap) * em,
            size: disc.size * em,
            gap: disc.gap * em,
        }
    }

    /// The middle and size of the next disc, whose count with its padding is `width` wide: a
    /// round disc, or a pill when the count is wider than a disc. None when it would reach the
    /// plaque's row, so that it and every disc after it are left out.
    pub fn next(&mut self, width: f32) -> Option<(Vec2, Vec2)> {
        if self.top - self.size < self.floor {
            return None;
        }
        let size = Vec2::new(width.max(self.size), self.size);
        let middle = Vec2::new(self.right - size.x / 2.0, self.top - size.y / 2.0);
        self.top -= self.size + self.gap;
        Some((middle, size))
    }
}
