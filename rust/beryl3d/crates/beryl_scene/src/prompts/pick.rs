//! The prompt shape `Pick`, objects picked one click at a time, and a prompt the session holds
//! no state for (`Candidates`), answered by the decision's candidates.
//!
//! A pick's candidates are clicked where they lie: a card on the table or in a hand through the
//! pointer (`Client::frame` sends `Click::On` for it), and a player through their life total,
//! which takes the press while the prompt marks them (`CountFloats`), as the web interface's
//! `PlayerTarget` (`ui/src/stage/PromptBar.tsx`) does. What may be picked and what is picked
//! wear the marks the session sends, and each target picked gets an arrow from the spell or
//! ability that asks, which the client begins and ends itself (`own`). Cards a pick lays out
//! off the table, such as the top of a library searched, are held up in the tray while it is
//! open, as the web's `trayOf` (`ui/src/stage/prompt.ts`) holds a pick's `tray`.
//!
//! A prompt the session holds no state for is not the table's to show, as the web's `onTable`
//! has it: it shows the prompt bar and draws nothing on the table.

use beryl_protocol::prompt::{PromptView, ShapeBody};
use beryl_protocol::protocol::{ObjectId, ZoneName};
use beryl_protocol::script::{Arrow, Mark, OwnStep, Subject};

use super::{Shown, Steps, begin_spans, end_spans, mark_spans, prompt_bar};
use crate::element::Element;
use crate::light::{ArrowSpan, MarkSpan, Span};
use crate::screens::ScreenIn;

/// What the flat layer shows for `prompt`: the prompt bar. A player the prompt marks is picked
/// by a press on their life total (`CountFloats`).
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    Some(prompt_bar(prompt, input))
}

/// Makes what the table shows for the prompt `shown` replace what it showed for `was`, as one
/// beat, as the web interface's `#mark` in `ui/src/stage/prompt.ts` plays it: the spans `was`
/// drew and `shown` does not end, then the tray lifts the cards of `shown` or lets those of
/// `was` go, then the spans new in `shown` begin. Allocates nothing.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    end_spans(spans, shown, was, steps);
    let (now, then) = (tray(shown), tray(was));
    if now != then {
        steps.tray(if now.is_empty() {
            OwnStep::TrayLetGo
        } else {
            OwnStep::TrayLifts {
                sorted: !then.is_empty(),
            }
        });
    }
    begin_spans(spans, shown, was, steps);
}

/// The cards `prompt`'s tray holds up, in order: none for a prompt that picks where they lie.
pub(crate) fn tray(prompt: Option<Shown<'_>>) -> &[ObjectId] {
    match prompt.map(|shown| &shown.prompt.body) {
        Some(ShapeBody::Pick { tray, .. }) => tray,
        _ => &[],
    }
}

/// The spans the table draws for `prompt`, as `Prompts.#mark` does: each of its marks; an arrow
/// from the spell or ability that asks to each target picked that the table shows an arrow to
/// once it is cast, a permanent or a player; and a suggested mark on each permanent that paying
/// for the card the pointer carries out of the hand would tap and nothing else of the prompt
/// marks, as Arena outlines the lands a spell held out of the hand would tap
/// (`iRr-GeSF0lI 20:34.4`). A prompt the session holds no state for draws none.
fn spans(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    let shown = prompt.filter(|shown| matches!(shown.prompt.body, ShapeBody::Pick { .. }));
    shown.into_iter().flat_map(|shown| {
        let marks = mark_spans(shown.prompt);
        let source = shown.source.filter(|&slot| shown.zone_of(slot).is_some());
        let arrows = shown.prompt.marks.iter().filter_map(move |marked| {
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
        });
        let carried = (shown.prompt.previews.iter())
            .filter(move |preview| Some(preview.card) == shown.carried)
            .flat_map(|preview| preview.taps.iter())
            .map(|&slot| Subject::Object { slot })
            .filter(move |on| !shown.prompt.marks.iter().any(|marked| marked.on == *on))
            .map(|on| {
                Span::Mark(MarkSpan {
                    mark: Mark::Suggested,
                    on,
                })
            });
        marks.chain(arrows).chain(carried)
    })
}
