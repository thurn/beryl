/// Which of a row's groups are in view: `shown` scrollable groups from `first`, with `before`
/// cards hidden at its left end and `after` at its right end. Groups too wide to show contribute
/// to the hidden counts. Its numbers are kept in `u16`s, which hold any row of a table, because a
/// window for each row is kept in every script's view at rest: as `usize`s they would make each
/// waiting script larger than 1024 bytes, past which a growing `Vec` of them reallocates more
/// often.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RowWindow {
    first: u16,
    shown: u16,
    last: u16,
    before: u16,
    after: u16,
}

impl RowWindow {
    /// A window of `shown` groups from `first`, with `last` as its maximum start.
    pub fn new(first: usize, shown: usize, last: usize, (before, after): (usize, usize)) -> Self {
        let small = |number: usize| u16::try_from(number).unwrap_or(u16::MAX);
        Self {
            first: small(first),
            shown: small(shown),
            last: small(last),
            before: small(before),
            after: small(after),
        }
    }

    /// The first group in view.
    #[must_use]
    pub fn first(&self) -> usize {
        usize::from(self.first)
    }

    /// How many groups are in view.
    #[must_use]
    pub fn shown(&self) -> usize {
        usize::from(self.shown)
    }

    /// The largest requested start that can move this window.
    #[must_use]
    pub fn last(&self) -> usize {
        usize::from(self.last)
    }

    /// How many cards are off the left end.
    #[must_use]
    pub fn before(&self) -> usize {
        usize::from(self.before)
    }

    /// How many cards are off the right end.
    #[must_use]
    pub fn after(&self) -> usize {
        usize::from(self.after)
    }

    /// Whether the row shows a window with cards off either end.
    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.shown > 0 && self.last > 0
    }

    /// The first group it may show: from 0 to the last from which every group to the end fits.
    #[must_use]
    pub fn clamp(&self, first: isize) -> usize {
        usize::try_from(first.max(0)).unwrap_or(0).min(self.last())
    }
}
