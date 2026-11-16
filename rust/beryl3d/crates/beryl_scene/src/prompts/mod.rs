//! Prompts: what the flat layer shows for the open prompt, and the marks and arrows the
//! client begins and ends itself while the player answers it (docs/beryl-3d.md, "The flat
//! layer"). Each group of shapes has a file of its own; both entry points are one match over
//! the prompt's `ShapeBody`.

mod bar;
mod catalog;
mod choose;
mod declare;
mod divide;
mod order;
mod pay;
mod pick;
mod split;

use beryl_protocol::prompt::{ButtonId, Marked, PromptView, ShapeBody};
use beryl_protocol::protocol::{ObjectId, PlayerView, ZoneName};
use beryl_protocol::script::{Mark, OwnStep, OwnTimings, StepTiming, Verb};

pub use bar::prompt_bar;

use crate::element::Element;
use crate::light::{Lit, MarkSpan, Span};
use crate::pose::Curve;
use crate::screens::ScreenIn;
use crate::verbs::Interval;

/// A prompt as the table shows it: its state, and whether the pointer is on the button that
/// shows what the prompt would tap, such as a payment's Auto Pay (`shows`). The web interface
/// keeps the second as `usePaying` in `ui/src/stage/PromptBar.tsx`. `source` is the spell or
/// ability the decision is for (`DecisionView::source`), which a target's arrow comes from;
/// `view` the latest view, which says where each object lies; and `carried` the card the
/// pointer carries out of the hand, whose payment's taps the table outlines.
#[derive(Clone, Copy, Debug)]
pub struct Shown<'a> {
    pub prompt: &'a PromptView,
    pub suggesting: bool,
    pub source: Option<ObjectId>,
    pub view: Option<&'a PlayerView>,
    pub carried: Option<ObjectId>,
}

impl Shown<'_> {
    /// Where the object in `slot` lies in the latest view, if it shows it.
    fn zone_of(&self, slot: ObjectId) -> Option<ZoneName> {
        let view = self.view?;
        (view.objects.iter())
            .find(|object| object.slot == slot)
            .map(|object| object.zone)
    }
}

/// What the flat layer shows for `prompt`, if anything.
#[must_use]
pub fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    match &prompt.body {
        ShapeBody::Pick { .. } | ShapeBody::Candidates => pick::screen(prompt, input),
        ShapeBody::Split { .. } => split::screen(prompt, input),
        ShapeBody::Order { .. } => order::screen(prompt, input),
        ShapeBody::Pay { .. } => pay::screen(prompt, input),
        ShapeBody::Declare { .. } => declare::screen(prompt, input),
        ShapeBody::YesNo { .. } | ShapeBody::Number { .. } | ShapeBody::Mode { .. } => {
            choose::screen(prompt, input)
        }
        ShapeBody::Catalog { .. } => catalog::screen(prompt, input),
        ShapeBody::Divide { .. } => divide::screen(prompt, input),
    }
}

/// Whether the pointer on `button` of `prompt`, or a touch held on it, shows what the prompt
/// would tap: a payment's Auto Pay. The web interface's `shows` in
/// `ui/src/stage/PromptBar.tsx` says the same.
#[must_use]
pub fn shows(prompt: &PromptView, button: ButtonId) -> bool {
    matches!(prompt.body, ShapeBody::Pay { .. }) && pay::shows(button)
}

/// Begins and ends the marks and arrows on the table that the client draws itself for the
/// prompt `shown`, which was `was` before (none when it has just opened), as one beat of
/// `steps`. `shown` is none when the prompt `was` has closed, which ends what it drew. Where
/// the two are of different shapes, the shape of `was` ends what it drew and then the shape of
/// `shown` draws.
pub fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    let shape = |prompt: Option<Shown>| prompt.and_then(|prompt| prompt.prompt.body.shape());
    if was.is_some() && shown.is_some() && shape(was) != shape(shown) {
        own_shape(None, was, steps);
        own_shape(shown, None, steps);
    } else {
        own_shape(shown, was, steps);
    }
}

/// `own` for a `shown` and `was` of one shape.
fn own_shape(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    let Some(body) = shown.or(was).map(|shown| &shown.prompt.body) else {
        return;
    };
    match body {
        ShapeBody::Pick { .. } | ShapeBody::Candidates => {
            pick::own(shown, was, steps);
        }
        ShapeBody::Split { .. } => split::own(shown, was, steps),
        ShapeBody::Order { .. } => order::own(shown, was, steps),
        ShapeBody::Pay { .. } => pay::own(shown, was, steps),
        ShapeBody::Declare { .. } => declare::own(shown, was, steps),
        ShapeBody::YesNo { .. } | ShapeBody::Number { .. } | ShapeBody::Mode { .. } => {
            choose::own(shown, was, steps);
        }
        ShapeBody::Catalog { .. } => catalog::own(shown, was, steps),
        ShapeBody::Divide { .. } => divide::own(shown, was, steps),
    }
}

/// Makes the marks on the table those of `shown` where they were those of `was`: the marks
/// `was` had and `shown` has not end, then the marks new in `shown` begin, one after another
/// in one beat, as the web interface's `#mark` in `ui/src/stage/prompt.ts` plays them. `look`
/// gives the mark the table draws for each of a prompt's marks, or none where it draws none.
/// Allocates nothing.
fn marks(
    shown: Option<Shown>,
    was: Option<Shown>,
    look: &impl Fn(Shown, Marked) -> Option<Mark>,
    steps: &mut Steps,
) {
    let drawn = |prompt| drawn(prompt, look);
    for span in drawn(was).filter(|span| !drawn(shown).any(|now| now == *span)) {
        let span = Span::Mark(span);
        let over = steps.next(OwnStep::MarkEnds, Verb::Mark, Some(&span));
        steps.lit.end(&span, over);
    }
    for span in drawn(shown).filter(|span| !drawn(was).any(|then| then == *span)) {
        let step = OwnStep::MarkBegins { mark: span.mark };
        let over = steps.next(step, Verb::Mark, None);
        steps.lit.begin(Span::Mark(span), over);
    }
}

/// The marks the table draws for `prompt`'s, by `look`.
fn drawn<'a>(
    prompt: Option<Shown<'a>>,
    look: &'a impl Fn(Shown, Marked) -> Option<Mark>,
) -> impl Iterator<Item = MarkSpan> + 'a {
    prompt.into_iter().flat_map(move |prompt| {
        (prompt.prompt.marks.iter()).filter_map(move |&marked| {
            look(prompt, marked).map(|mark| MarkSpan {
                mark,
                on: marked.on,
            })
        })
    })
}

/// The steps a prompt makes itself in one call of `own`, played as one beat from `now`, in
/// nanoseconds since the client started, on the spans of `lit`, each timed by the session's
/// entry for its shape in `timings` (at once, taking no time, where it gave none). A beat is
/// laid out as `beat::lay` lays a script's: a step begins its delay after its turn comes and
/// never before the step before it, the next step's turn comes a stagger after, and a step
/// that ends a span waits until the span has stood whole for its hold. None of these steps
/// moves anything, plays `with` the step before it or waits for the table, so the rest of
/// `beat::lay` has nothing to do here. Where `record` is given, each step is written to its
/// list with its view.
pub struct Steps<'a> {
    lit: &'a mut Lit,
    timings: &'a OwnTimings,
    now: u64,
    /// When the next step's turn comes and when the last step began, in milliseconds.
    turn: f64,
    from: f64,
    played: usize,
    record: Option<(usize, &'a mut Vec<OwnRecord>)>,
    /// The tray's step, if one of the steps lifts the prompt's cards into its tray or lets them
    /// go: its shape, when it plays, and its curve. The client moves the cards (`Table::lift`).
    tray: Option<(OwnStep, Interval, Curve)>,
}

/// One step a prompt made itself, as the timeline prints it: the number of views the client
/// had been given when it played, its place in its beat, its verb, the timing the session gave
/// it, if any, and when it began and ended, in nanoseconds from the start of its beat.
#[derive(Clone, Debug, PartialEq)]
pub struct OwnRecord {
    pub view: usize,
    pub index: usize,
    pub verb: Verb,
    pub timing: Option<StepTiming>,
    pub from: u64,
    pub to: u64,
}

/// Nanoseconds in a millisecond.
const NS_PER_MS: f64 = 1_000_000.0;

impl<'a> Steps<'a> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a client's clock stays far below 2^52 nanoseconds"
    )]
    pub fn new(
        lit: &'a mut Lit,
        timings: &'a OwnTimings,
        now: u64,
        record: Option<(usize, &'a mut Vec<OwnRecord>)>,
    ) -> Self {
        let at = now as f64 / NS_PER_MS;
        Self {
            lit,
            timings,
            now,
            turn: at,
            from: at,
            played: 0,
            record,
            tray: None,
        }
    }

    /// The beat's next step lifts the prompt's cards into its tray, or lets them go: `step` is
    /// `TrayLifts` or `TrayLetGo`.
    fn tray(&mut self, step: OwnStep) {
        let over = self.next(step, Verb::Lift, None);
        let curve = (self.timings.entries.iter())
            .find(|entry| entry.step == step)
            .map_or(Curve::EASE_OUT, |entry| Curve::named(entry.timing.curve));
        self.tray = Some((step, over, curve));
    }

    /// The tray's step, if the beat has one (`tray`).
    #[must_use]
    pub fn tray_step(&self) -> Option<(OwnStep, Interval, Curve)> {
        self.tray
    }

    /// When the next step, of the shape `step`, plays, in nanoseconds since the client started;
    /// `ends` is the span it ends, if it ends one.
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "times are positive and far below 2^52 nanoseconds"
    )]
    fn next(&mut self, step: OwnStep, verb: Verb, ends: Option<&Span>) -> Interval {
        let timing = (self.timings.entries.iter())
            .find(|entry| entry.step == step)
            .map(|entry| &entry.timing);
        let over = if let Some(timing) = timing {
            let held = timing
                .hold
                .zip(ends.and_then(|span| self.lit.whole_since(span)))
                .map_or(f64::NEG_INFINITY, |(hold, since)| {
                    since as f64 / NS_PER_MS + hold
                });
            self.from = self.from.max(self.turn.max(held) + timing.delay);
            self.turn += timing.stagger;
            Interval {
                from: (self.from * NS_PER_MS) as u64,
                to: ((self.from + timing.duration) * NS_PER_MS) as u64,
            }
        } else {
            let at = (self.from * NS_PER_MS) as u64;
            Interval { from: at, to: at }
        };
        if let Some((view, record)) = &mut self.record {
            record.push(OwnRecord {
                view: *view,
                index: self.played,
                verb,
                timing: timing.cloned(),
                from: over.from.saturating_sub(self.now),
                to: over.to.saturating_sub(self.now),
            });
        }
        self.played += 1;
        over
    }
}
