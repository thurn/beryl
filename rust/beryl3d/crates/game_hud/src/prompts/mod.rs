//! Prompts: what the flat layer shows for the open prompt, and the marks and arrows the
//! client begins and ends itself while the player answers it (docs/beryl-3d.md, "The flat
//! layer"). Each group of shapes has a file of its own; both entry points are one match over
//! the prompt's `ShapeBody`.

pub mod bar;
mod catalog;
mod choose;
mod declare;
mod divide;
pub mod order;
mod pay;
mod pick;
mod split;
mod tray;

use beryl_protocol::prompt::{ButtonId, Marked, PromptButton, PromptView, Purpose, ShapeBody};
use beryl_protocol::protocol::{ObjectId, PlayerView, ZoneName};
use beryl_protocol::script::{Arrow, Mark, OwnStep, OwnTimings, StepTiming, Subject, Verb};

pub use bar::{prompt_bar, waiting};
pub use tray::Piles;

use crate::hud::HudIn;
use game_play::light::{ArrowSpan, Lit, MarkSpan, Span};
use game_play::verbs::Interval;
use kit_base::pose::Curve;
use kit_ui::element::Element;
use kit_ui::input::FaceId;

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

/// What the flat layer shows for `prompt`, if anything. While the prompt holds cards up in its
/// tray, the table lies under the tray's shade, first in the flat layer so that the bar is drawn
/// over it, and the cards held up are drawn over the flat layer (`Table::in_tray`), as the web
/// interface's `TrayUnder` shades the table under them (`shades` in `ui/src/stage/Tray.tsx`).
#[must_use]
pub fn screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    let mut screen = shape_screen(prompt, input)?;
    if Piles::of(prompt).is_some() {
        screen.children.insert(0, shade(input));
    }
    Some(screen)
}

/// What the flat layer shows for the shape of `prompt`, if anything.
fn shape_screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
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

/// The whole window in the tray's shade (`.stage-tray`), which the pointer goes through.
fn shade(input: &HudIn) -> Element {
    let window = input.window.size;
    let mut shade = kit_ui::element::column()
        .name("shade")
        .width(window.width)
        .height(window.height)
        .background(kit_base::tokens::color::TRAY_SHADE)
        .passes(true);
    shade.anchor = Some(kit_ui::element::Anchor::Point(
        glam::Vec2::ZERO,
        glam::Vec2::ZERO,
    ));
    shade
}

/// The faces the open prompt shows in the flat layer at full size, which the client asks for
/// at full size (`Faces::want_full`): those of the ways to use a card (`choose`).
#[must_use]
pub fn faces(input: &HudIn) -> Vec<FaceId> {
    input.prompt.map_or_else(Vec::new, |prompt| {
        choose::way_faces(prompt, input).flatten().collect()
    })
}

/// Whether `prompt` lists what it asks about over the table on a shade of its own, so that the
/// table under it takes no press, as the web interface's `coversTable` in
/// `ui/src/stage/prompt.ts` has it for an order's waiting triggers and a catalog's entries. A
/// prompt that holds cards up in its tray covers the table through the tray
/// (`Table::pressable`).
#[must_use]
pub fn covers_table(prompt: &PromptView) -> bool {
    matches!(
        prompt.body,
        ShapeBody::Order { .. } | ShapeBody::Catalog { .. }
    )
}

/// Whether the pointer on `button` of `prompt`, or a touch held on it, shows what the prompt
/// would tap: a payment's Auto Pay, or the yes of a question whose yes pays a cost. The web
/// interface's `shows` in `ui/src/stage/PromptBar.tsx` says the same.
#[must_use]
pub fn shows(prompt: &PromptView, button: ButtonId) -> bool {
    match prompt.body {
        ShapeBody::Pay { .. } => pay::shows(button),
        ShapeBody::YesNo { .. } => choose::shows(prompt, button),
        _ => false,
    }
}

/// Whether the table shows anything of `prompt`: every prompt but what the opening screen
/// asks, who plays first and whether to keep a hand, as the web's `onTable`
/// (`ui/src/stage/prompt.ts`) has it.
pub(super) fn on_table(prompt: &PromptView) -> bool {
    !(prompt.is(Purpose::PlayOrDraw) || prompt.is(Purpose::Mulligan))
}

/// The buttons of `prompt` that are the ways to use a card, where it is the choice among them
/// (a mode of purpose `act`): each way the card can be cast, played or activated, and each
/// activated ability it cannot use now. The web's `waysOf` (`ui/src/stage/prompt.ts`). These
/// stand over the table as the card once a way (`choose`), not in the prompt bar.
pub fn ways(prompt: &PromptView) -> impl Iterator<Item = &PromptButton> {
    let choosing = matches!(prompt.body, ShapeBody::Mode { .. }) && prompt.is(Purpose::Act);
    (prompt.buttons.iter())
        .filter(move |_| choosing)
        .filter(|button| {
            matches!(
                button.id,
                ButtonId::Option { .. } | ButtonId::Ability { .. }
            )
        })
}

/// Begins and ends the marks and arrows on the table that the client draws itself for the
/// prompt `shown`, which was `was` before (none when it has just opened), as one beat of
/// `steps`. `shown` is none when the prompt `was` has closed, which ends what it drew. Where
/// the two are of different shapes, the shape of `was` ends what it drew and then the shape of
/// `shown` draws.
pub fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    let showing = |prompt: &Shown| on_table(prompt.prompt);
    let (shown, was) = (shown.filter(showing), was.filter(showing));
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
/// `was` had and `shown` has not end, then the tray lifts the cards `shown` holds up or lets
/// those of `was` go (`tray::lift`), then the marks new in `shown` begin, one after another in one
/// beat, as the web interface's `#mark` in `ui/src/stage/prompt.ts` plays them. A step is
/// played for each mark as the session sent it, so that both clients' timelines hold the same
/// steps. `look` gives the mark the table draws for each of a prompt's marks, or none where it
/// draws none, as the web stage's styles draw a mark by the prompt's state (`data-pay` in
/// `ui/src/stage/stage.css`): what a mark's step begins or ends is the mark as drawn. Where
/// only the look of a mark changed (the pointer came onto Auto Pay), the web plays no step and
/// its styles change at once, so the table's spans change at once too. Allocates nothing.
fn marks<'a>(
    shown: Option<Shown<'a>>,
    was: Option<Shown<'a>>,
    look: &impl Fn(Shown, Marked) -> Option<Mark>,
    steps: &mut Steps,
) {
    let has = |prompt: Option<Shown<'a>>, marked: &Marked| sent(prompt).any(|then| then == marked);
    let spans = |prompt| drawn(prompt, look);
    let looked = |prompt: Option<Shown<'a>>, marked: Marked| {
        let mark = look(prompt?, marked)?;
        Some(Span::Mark(MarkSpan {
            mark,
            on: marked.on,
        }))
    };
    let draws = |prompt, span: &Span| spans(prompt).any(|now| Span::Mark(now) == *span);
    for &marked in sent(was).filter(|marked| !has(shown, marked)) {
        let span = looked(was, marked);
        let over = steps.next(OwnStep::MarkEnds, Verb::Mark, span.as_ref());
        if let Some(span) = span.filter(|span| !draws(shown, span)) {
            steps.lit.end(&span, over);
        }
    }
    tray::lift(shown, was, steps);
    for &marked in sent(shown).filter(|marked| !has(was, marked)) {
        let step = OwnStep::MarkBegins { mark: marked.mark };
        let over = steps.next(step, Verb::Mark, None);
        if let Some(span) = looked(shown, marked).filter(|span| !draws(was, span)) {
            steps.lit.begin(span, over);
        }
    }
    let at_once = Interval {
        from: steps.now,
        to: steps.now,
    };
    for &marked in sent(shown).filter(|marked| has(was, marked)) {
        let (then, now) = (looked(was, marked), looked(shown, marked));
        if then == now {
            continue;
        }
        if let Some(span) = then.filter(|span| !draws(shown, span)) {
            steps.lit.end(&span, at_once);
        }
        if let Some(span) = now.filter(|span| !draws(was, span)) {
            steps.lit.begin(span, at_once);
        }
    }
}

/// The marks the session sent with `prompt`.
fn sent(prompt: Option<Shown<'_>>) -> impl Iterator<Item = &Marked> {
    prompt
        .into_iter()
        .flat_map(|shown| shown.prompt.marks.iter())
}

/// Ends each span that `spans` gives for `was` and not for `shown`, an arrow by `ArrowEnds` and
/// anything else by `MarkEnds`, one after another in the beat of `steps`, as the web
/// interface's `#mark` in `ui/src/stage/prompt.ts` ends them. Allocates nothing.
fn end_spans<'a, I: Iterator<Item = Span>>(
    spans: impl Fn(Option<Shown<'a>>) -> I,
    shown: Option<Shown<'a>>,
    was: Option<Shown<'a>>,
    steps: &mut Steps,
) {
    for span in spans(was).filter(|span| !spans(shown).any(|now| now == *span)) {
        let (step, verb) = match span {
            Span::Arrow(_) => (OwnStep::ArrowEnds, Verb::Point),
            _ => (OwnStep::MarkEnds, Verb::Mark),
        };
        let over = steps.next(step, verb, Some(&span));
        steps.lit.end(&span, over);
    }
}

/// Begins each arrow and mark that `spans` gives for `shown` and not for `was`, one after
/// another in the beat of `steps`, as `#mark` begins them. Allocates nothing.
fn begin_spans<'a, I: Iterator<Item = Span>>(
    spans: impl Fn(Option<Shown<'a>>) -> I,
    shown: Option<Shown<'a>>,
    was: Option<Shown<'a>>,
    steps: &mut Steps,
) {
    for span in spans(shown).filter(|span| !spans(was).any(|then| then == *span)) {
        let (step, verb) = match span {
            Span::Arrow(ArrowSpan { arrow, .. }) => (OwnStep::ArrowBegins { arrow }, Verb::Point),
            Span::Mark(MarkSpan { mark, .. }) => (OwnStep::MarkBegins { mark }, Verb::Mark),
            _ => continue,
        };
        let over = steps.next(step, verb, None);
        steps.lit.begin(span, over);
    }
}

/// Each of `prompt`'s marks as a span.
fn mark_spans(prompt: &PromptView) -> impl Iterator<Item = Span> + '_ {
    prompt.marks.iter().map(|marked| {
        Span::Mark(MarkSpan {
            mark: marked.mark,
            on: marked.on,
        })
    })
}

/// An arrow from the spell or ability that asks to each target the prompt `shown` marks, where
/// the table shows an arrow to it once the spell is cast: a permanent or a player, as the web
/// interface's `#mark` in `ui/src/stage/prompt.ts` draws one for each `AIMED` mark. None where
/// the source is not on the table.
fn target_arrows(shown: Shown<'_>) -> impl Iterator<Item = Span> + '_ {
    let source = shown.source.filter(|&slot| shown.zone_of(slot).is_some());
    shown.prompt.marks.iter().filter_map(move |marked| {
        let arrow = match marked.mark {
            Mark::FriendlyTarget => Arrow::FriendlyTarget,
            Mark::HostileTarget => Arrow::HostileTarget,
            _ => return None,
        };
        let there = match marked.on {
            Subject::Player { .. } => true,
            Subject::Object { slot } => shown.zone_of(slot) == Some(ZoneName::Battlefield),
            Subject::Ability { .. } => false,
        };
        let from = Subject::Object { slot: source? };
        there.then_some(Span::Arrow(ArrowSpan {
            arrow,
            from,
            to: marked.on,
        }))
    })
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
/// had been given when it played, its beat and its place in it, its verb, the timing the
/// session gave it, if any, and when it began and ended, in nanoseconds from the start of its
/// beat. A closed prompt's tray letting its cards go is the exception: it is written under the
/// view whose script answered the prompt, first in the beat of that script it plays in, with
/// times on that script's clock (`Show::place_let_go`).
#[derive(Clone, Debug, PartialEq)]
pub struct OwnRecord {
    pub view: usize,
    pub beat: usize,
    pub index: usize,
    pub verb: Verb,
    pub timing: Option<StepTiming>,
    pub from: u64,
    pub to: u64,
}

/// Nanoseconds in a millisecond.
#[allow(clippy::cast_lossless, reason = "`From` is not `const`")]
const NS_PER_MS: f64 = kit_base::tokens::ratio::NANOS_PER_MS as f64;

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
            .map_or(Curve::EASE_OUT, |entry| {
                game_play::named_curve(entry.timing.curve)
            });
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
                .map_or(f64::NEG_INFINITY, |(hold, since)| since / NS_PER_MS + hold);
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
                beat: 0,
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
