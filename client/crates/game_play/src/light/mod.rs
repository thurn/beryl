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

use crate::verbs::Interval;
use kit_base::pose::Curve;

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
    /// When it stood whole on the script clock (`Lit::ahead`), from which a hold is measured.
    whole: u64,
}

/// An arrow or line to the pointer, as the web interface's `Aim` (`ui/src/stage/Aim.tsx`) draws
/// it: a choice of targets draws one from the spell or ability that asks, until as many
/// targets are picked as it takes (UI Conventions, "An arrow to the pointer"), and a
/// declaration one from each creature declared that awaits what it is aimed at (UI
/// Conventions, "Attacking a planeswalker"). It is the pointer's trace, not a span: it has no
/// step and no fade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub arrow: Arrow,
    pub from: Subject,
    /// Where the pointer is, in logical points of the window.
    pub to: glam::Vec2,
}

/// Every span and streak the table shows.
///
/// A step that ends a span may wait until the span has stood whole for its `hold`, and the web
/// interface measures that hold in script time. the client plays scripts faster than real time
/// when updates pile up (`data/client/catch-up.json`), so `Lit` keeps a script
/// clock beside real time: it runs as fast as each script played, and at real time between
/// scripts. `ahead` is how far the script clock is ahead of real time once the last script
/// ended, and each span records when it stood whole on that clock.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lit {
    open: Vec<Open>,
    streaks: Vec<StreakRun>,
    /// The arrows to the pointer drawn now (`Aim`).
    aims: Vec<Aim>,
    /// How many nanoseconds the script clock is ahead of real time between scripts: the sum,
    /// over every script played so far, of how much shorter it took in real time than on its
    /// own clock.
    ahead: u64,
}

impl Lit {
    /// No span or streak, with room for `capacity` of each, so that holding that many
    /// allocates nothing.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            open: Vec::with_capacity(capacity),
            streaks: Vec::with_capacity(capacity),
            aims: Vec::with_capacity(capacity),
            ahead: 0,
        }
    }

    /// The arrows to the pointer drawn now.
    #[must_use]
    pub fn aims(&self) -> &[Aim] {
        &self.aims
    }

    /// Draws `aims` to the pointer in place of those drawn before. Allocates only when there
    /// are more of them than the capacity `Lit` was made with.
    pub fn set_aims(&mut self, aims: impl IntoIterator<Item = Aim>) {
        self.aims.clear();
        self.aims.extend(aims);
    }

    /// `span` begins to appear, standing whole once `over` is done. A revealed face joins the
    /// Revealed Cards pile on top: the faces another cause revealed, and the face its own card
    /// showed there before, leave the pile as it begins, as the web interface's `heldUp`
    /// (`ui/src/stage/table.ts`) has it.
    pub fn begin(&mut self, span: Span, over: Interval) {
        self.open_span(span, over, over.to + self.ahead);
    }

    /// `span` begins to appear over `over`, in nanoseconds since the client started, as a step
    /// of the script that started at `start` writes it, standing whole at `whole` on that
    /// script's clock. The script runs faster than real time while it catches up, and the
    /// span's hold is measured on the script's clock (`whole_since`).
    pub fn begin_in_script(&mut self, span: Span, over: Interval, start: u64, whole: u64) {
        self.open_span(span, over, start + self.ahead + whole);
    }

    /// A script that played `ran` nanoseconds on its own clock has ended, after `took`
    /// nanoseconds of real time: the script clock is ahead of real time by the difference more.
    pub fn script_ended(&mut self, ran: u64, took: u64) {
        self.ahead += ran.saturating_sub(took);
    }

    fn open_span(&mut self, span: Span, over: Interval, whole: u64) {
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
            whole,
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

    /// Since when `span` has stood whole, while it is open and not going, for a step that ends
    /// it after a hold: in nanoseconds of the real time between scripts, as if every script
    /// before had played at real time. A step that plays between scripts, or at the start of
    /// a script, waits until this time plus its hold, whatever rate the scripts that played
    /// since the span stood whole ran at. It is below zero for a span that stood whole on the
    /// script clock less time after the client started than the clock has since gained.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        reason = "a client runs for hours, far below f64's exact range in nanoseconds"
    )]
    pub fn whole_since(&self, span: &Span) -> Option<f64> {
        self.open
            .iter()
            .find(|open| open.ends.is_none() && open.span == *span)
            .map(|open| open.whole as f64 - self.ahead as f64)
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
