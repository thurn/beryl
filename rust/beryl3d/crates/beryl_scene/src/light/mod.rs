//! Light: the arrows, marks, lifts and streaks the table shows over its cards (docs/beryl-3d.md,
//! "Effects"). A verb writes them as records when its script is compiled (`SpanChange`,
//! `StreakRun`), the show applies each record to its `Lit` once the script's clock reaches it,
//! and the writers in `arrows`, `marks` and `streaks` draw what `Lit` holds.
//!
//! A span is shown from the step that begins it until the step that ends it, across scripts
//! (`beryl_protocol::script`, "Spans"). A script with a reset drops every span.

pub mod arrows;
pub mod lines;
pub mod marks;
pub mod streaks;

use beryl_protocol::protocol::{CardHandling, ObjectId};
use beryl_protocol::script::{self, Arrow, Mark, Stream, Subject};

use crate::pose::Curve;
use crate::verbs::Interval;

/// An arrow or line from one thing to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrowSpan {
    pub arrow: Arrow,
    pub from: Subject,
    pub to: Subject,
}

/// A mark on a card, an ability or a player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkSpan {
    pub mark: Mark,
    pub on: Subject,
}

/// Something the table shows from when it begins to when it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Arrow(ArrowSpan),
    Mark(MarkSpan),
    /// Cards the opponent is handling, lifted from their pile.
    Lift(CardHandling),
    /// The face of the card in `slot`, held up for both players to read in the Revealed Cards
    /// pile, as `cause` revealed it (`showcase`). The table keeps the record of the face shown
    /// (`Table::reveal`), since the card itself may since have gone where its name is hidden.
    Revealed {
        slot: ObjectId,
        cause: Option<Subject>,
    },
    /// The spell, ability or player a `dwell` step leaves to be read, for as long as the step
    /// plays: drawn `focused`.
    Focus(Subject),
}

/// A span begins or ends, fading over `over` on the script's clock. `step` is the number of the
/// step that wrote it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpanChange {
    pub span: Span,
    pub end: script::Span,
    pub over: Interval,
    pub step: usize,
}

/// Mana or damage streaming from one thing to another over `over`, on the script's clock once
/// written and in nanoseconds since the client started once the show applies it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreakRun {
    pub stream: Stream,
    pub from: Option<Subject>,
    pub to: Subject,
    pub amount: u8,
    pub over: Interval,
    pub curve: Curve,
    pub step: usize,
}

/// One span the table shows: when it began to appear, when it stood whole, and when it began and
/// finished going, in nanoseconds since the client started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Open {
    pub span: Span,
    pub begins: Interval,
    pub ends: Option<Interval>,
}

/// Every span and streak the table shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lit {
    open: Vec<Open>,
    streaks: Vec<StreakRun>,
}

impl Lit {
    /// No span or streak, with room for `capacity` of each, so that holding that many
    /// allocates nothing.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            open: Vec::with_capacity(capacity),
            streaks: Vec::with_capacity(capacity),
        }
    }

    /// `span` begins to appear, standing whole once `over` is done. A revealed face joins the
    /// Revealed Cards pile on top: the faces another cause revealed, and the face its own card
    /// showed there before, leave the pile as it begins, as the web interface's `heldUp`
    /// (`ui/src/stage/table.ts`) has it.
    pub fn begin(&mut self, span: Span, over: Interval) {
        if let Span::Revealed { slot, cause } = span {
            for open in &mut self.open {
                if let Span::Revealed {
                    slot: shown,
                    cause: by,
                } = open.span
                    && open.ends.is_none()
                    && (shown == slot || by != cause)
                {
                    open.ends = Some(Interval {
                        from: over.from,
                        to: over.from,
                    });
                }
            }
        }
        self.open.push(Open {
            span,
            begins: over,
            ends: None,
        });
    }

    /// The open span equal to `span` begins to go, and is gone once `over` is done. A span that
    /// is not open, or already going, is left as it is.
    pub fn end(&mut self, span: &Span, over: Interval) {
        if let Some(open) = self
            .open
            .iter_mut()
            .find(|open| open.ends.is_none() && open.span == *span)
        {
            open.ends = Some(over);
        }
    }

    /// Every span shown, in the order they began.
    #[must_use]
    pub fn open(&self) -> &[Open] {
        &self.open
    }

    /// Since when `span` has stood whole, for `beat`'s `whole`: the end of its appearing, while
    /// it is open and not going.
    #[must_use]
    pub fn whole_since(&self, span: &Span) -> Option<u64> {
        self.open
            .iter()
            .find(|open| open.ends.is_none() && open.span == *span)
            .map(|open| open.begins.to)
    }

    /// A streak begins, over `run.over` in nanoseconds since the client started.
    pub fn streak(&mut self, run: StreakRun) {
        self.streaks.push(run);
    }

    /// Every streak under way or not yet forgotten, in the order they began.
    #[must_use]
    pub fn streaks(&self) -> &[StreakRun] {
        &self.streaks
    }

    /// Drops every span and streak, as a script with a reset does.
    pub fn clear(&mut self) {
        self.open.clear();
        self.streaks.clear();
    }

    /// Forgets the spans that finished going and the streaks that ended before `now`. Allocates
    /// nothing.
    pub fn advance(&mut self, now: u64) {
        self.open
            .retain(|open| open.ends.is_none_or(|ends| ends.to >= now));
        self.streaks.retain(|run| run.over.to >= now);
    }
}
