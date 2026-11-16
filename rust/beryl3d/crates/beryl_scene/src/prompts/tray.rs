//! The cards a prompt holds up off the table in its tray, as the web interface's `trayOf`
//! (`ui/src/stage/prompt.ts`) lists them: a pick's tray as one pile, such as the top of a
//! library searched, and a split's two piles, each laid out for as many cards as it must come to
//! hold. And the one beat in which a prompt that holds cards up changes what the table shows,
//! as the web's `Prompts.#mark` plays it.

use beryl_protocol::prompt::{PromptView, ShapeBody};
use beryl_protocol::protocol::ObjectId;
use beryl_protocol::script::OwnStep;

use super::{Shown, Steps, begin_spans, end_spans, mark_spans};
use crate::light::Span;
use crate::table::Held;

/// The cards a prompt holds up, pile by pile, borrowed from its state, so that comparing two
/// states allocates nothing: one pile or two, and how many cards each of two must come to hold,
/// where the prompt says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piles<'a> {
    cards: [&'a [ObjectId]; 2],
    rooms: [Option<u8>; 2],
    count: usize,
}

impl<'a> Piles<'a> {
    /// The cards `prompt` holds up: a pick's non-empty tray, or a split's two piles. None for any
    /// other prompt, which picks what it asks about where it lies.
    #[must_use]
    pub fn of(prompt: &'a PromptView) -> Option<Self> {
        match &prompt.body {
            ShapeBody::Pick { tray, .. } if !tray.is_empty() => Some(Self {
                cards: [tray, &[]],
                rooms: [None; 2],
                count: 1,
            }),
            ShapeBody::Split { piles } => match &piles[..] {
                [first, second] => Some(Self {
                    cards: [&first.cards, &second.cards],
                    rooms: [first.exactly, second.exactly],
                    count: 2,
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// The piles as the table holds them up.
    #[must_use]
    pub fn held(&self) -> Held {
        Held {
            piles: self.cards[..self.count]
                .iter()
                .map(|pile| pile.to_vec())
                .collect(),
            rooms: self.rooms[..self.count]
                .iter()
                .map(|room| room.map(usize::from))
                .collect(),
        }
    }
}

/// Makes what the table shows for the prompt `shown` replace what it showed for `was`, as one
/// beat, as the web interface's `#mark` plays it: the spans `was` drew (`spans`) and `shown`
/// does not end, then the tray lifts the cards of `shown` (sorted, where a tray was up already)
/// or lets those of `was` go, then the spans new in `shown` begin. Allocates nothing.
pub(super) fn own<'s, I>(
    shown: Option<Shown<'s>>,
    was: Option<Shown<'s>>,
    spans: impl Fn(Option<Shown<'s>>) -> I,
    steps: &mut Steps,
) where
    I: Iterator<Item = Span>,
{
    end_spans(&spans, shown, was, steps);
    let piles = |prompt: Option<Shown<'s>>| prompt.and_then(|shown| Piles::of(shown.prompt));
    let (now, then) = (piles(shown), piles(was));
    if now != then {
        steps.tray(if now.is_none() {
            OwnStep::TrayLetGo
        } else {
            OwnStep::TrayLifts {
                sorted: then.is_some(),
            }
        });
    }
    begin_spans(&spans, shown, was, steps);
}

/// Each mark `prompt` sends, as the table draws it.
pub(super) fn marks(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    (prompt.into_iter()).flat_map(|shown| mark_spans(shown.prompt))
}
