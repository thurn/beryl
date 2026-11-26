//! The cards a prompt holds up off the table in its tray, as the web interface's `trayOf`
//! (`ui/src/stage/prompt.ts`) lists them: a pick's tray as one pile, such as the top of a
//! library searched, a split's two piles, each laid out for as many cards as it must come to
//! hold, marked as a put-back where the cards go from the hand onto the library, or as a pick
//! where some are taken out of the library, and the cards a yes or no or a choice among answers
//! is about, as one pile. And the one beat in which a prompt that holds cards up changes what
//! the table shows,
//! as the web's `Prompts.#mark` plays it.

use beryl_protocol::prompt::{Pile, PlaceName, PromptView, ShapeBody};
use beryl_protocol::protocol::ObjectId;
use beryl_protocol::script::OwnStep;

use super::{Shown, Steps, begin_spans, end_spans, mark_spans, on_table, ways};
use game_play::light::Span;
use game_table::table::{Held, TrayKind};
use glam::Vec2;
use kit_base::tokens::color;
use kit_text::text::TypeStyle;
use kit_ui::element::{Anchor, Element, Justify, row, text};

/// The cards a prompt holds up, pile by pile, borrowed from its state, so that comparing two
/// states allocates nothing: one pile or two, how many cards each of two must come to hold,
/// where the prompt says, and how two are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piles<'a> {
    cards: [&'a [ObjectId]; 2],
    rooms: [Option<u8>; 2],
    /// The index of each held pile in `ShapeBody::Split::piles` (identity `[0, 1]` when both
    /// piles are shown, or `[1, 0]` with `count == 1` when the first pile is closed and empty).
    indices: [usize; 2],
    count: usize,
    kind: Kind,
}

/// How a tray of piles is laid out (`TrayKind`), without a fan's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Boxes,
    SingleBox,
    Fan,
    PutBack,
}

impl Kind {
    /// How a split into `onto` and `from` is laid out, as the web's `trayOf` decides: a fan
    /// while the first pile takes cards out of the library to somewhere else and the second
    /// leaves them on the library (a look that puts two of them into the hand: Arena shows one
    /// fan), until the first pile is closed and what is left is ordered in its box; a put-back
    /// where the first pile puts cards on top of the library and the second leaves them where
    /// they lie, in the hand; boxes for any other.
    fn of(onto: &Pile, from: &Pile) -> Self {
        let taken = matches!(
            onto.place,
            Some(
                PlaceName::Hand | PlaceName::Graveyard | PlaceName::Exile | PlaceName::Battlefield
            )
        );
        let left = matches!(from.place, Some(PlaceName::Top | PlaceName::Bottom));
        if !onto.closed && taken && left {
            Self::Fan
        } else if onto.place == Some(PlaceName::OnTop) && from.place.is_none() {
            Self::PutBack
        } else {
            Self::Boxes
        }
    }
}

impl<'a> Piles<'a> {
    /// The cards `prompt` holds up: a pick's non-empty tray, a split's two piles, or the cards
    /// a yes or no or a choice among answers is about, unless the choice is among the ways to
    /// use a card, which shows the card once a way instead (`super::mode`). None for any other
    /// prompt, which picks what it asks about where it lies, or for one the table does not show
    /// (`super::on_table`).
    #[must_use]
    pub fn of(prompt: &'a PromptView) -> Option<Self> {
        if !on_table(prompt) {
            return None;
        }
        let one = |cards: &'a [ObjectId]| Self {
            cards: [cards, &[]],
            rooms: [None; 2],
            indices: [0, 1],
            count: 1,
            kind: Kind::Boxes,
        };
        let closed_empty = |pile: &Pile| pile.closed && pile.cards.is_empty();
        match &prompt.body {
            ShapeBody::Pick { tray, .. } if !tray.is_empty() => Some(one(tray)),
            ShapeBody::YesNo { about, .. } if !about.is_empty() => Some(one(about)),
            ShapeBody::Mode { about } if !about.is_empty() && ways(prompt).next().is_none() => {
                Some(one(about))
            }
            ShapeBody::Split { piles } => match &piles[..] {
                [first, second] if closed_empty(first) || closed_empty(second) => {
                    let (shown, index) = if closed_empty(first) {
                        (second, 1)
                    } else {
                        (first, 0)
                    };
                    Some(Self {
                        cards: [&shown.cards, &[]],
                        rooms: [shown.exactly, None],
                        indices: [index, 1 - index],
                        count: 1,
                        kind: Kind::SingleBox,
                    })
                }
                [first, second] => Some(Self {
                    cards: [&first.cards, &second.cards],
                    rooms: [first.exactly, second.exactly],
                    indices: [0, 1],
                    count: 2,
                    kind: Kind::of(first, second),
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// The index in `ShapeBody::Split::piles` for each pile held in the tray.
    #[must_use]
    pub fn pile_indices(&self) -> &[usize] {
        &self.indices[..self.count]
    }

    /// The piles as the table holds them up. A fan keeps the order of `up`, the tray held up
    /// until now, for the cards still in it, so that a card clicked stays where it lies; the
    /// cards new to it follow, those left in the library first, as they were dealt.
    #[must_use]
    pub fn held(&self, up: Option<&Held>) -> Held {
        let piles = &self.cards[..self.count];
        let kind = match self.kind {
            Kind::Boxes => TrayKind::Boxes,
            Kind::SingleBox => TrayKind::SingleBox,
            Kind::PutBack => TrayKind::PutBack,
            Kind::Fan => {
                let all = || piles.iter().rev().flat_map(|pile| pile.iter().copied());
                let kept: &[ObjectId] = match up.map(|up| &up.kind) {
                    Some(TrayKind::Fan { order }) => order,
                    _ => &[],
                };
                let mut order: Vec<ObjectId> = (kept.iter().copied())
                    .filter(|card| all().any(|dealt| dealt == *card))
                    .collect();
                let new: Vec<ObjectId> = all().filter(|card| !order.contains(card)).collect();
                order.extend(new);
                TrayKind::Fan { order }
            }
        };
        Held {
            piles: piles.iter().map(|pile| pile.to_vec()).collect(),
            rooms: self.rooms[..self.count]
                .iter()
                .map(|room| room.map(usize::from))
                .collect(),
            kind,
        }
    }

    /// Whether the piles are held up as the library and the hand a put-back takes cards from
    /// and puts them on, rather than in boxes.
    #[must_use]
    pub fn put_back(&self) -> bool {
        self.kind == Kind::PutBack
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
    lift(shown, was, steps);
    begin_spans(&spans, shown, was, steps);
}

/// Lifts the cards `shown` holds up into the tray (sorted, where a tray was up already) or
/// lets those `was` held go, as one step of `steps`, where the two hold up different cards.
pub(super) fn lift<'s>(shown: Option<Shown<'s>>, was: Option<Shown<'s>>, steps: &mut Steps) {
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
}

/// Each mark `prompt` sends, as the table draws it.
pub(super) fn marks(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    (prompt.into_iter()).flat_map(|shown| mark_spans(shown.prompt))
}

/// The session's words for the left and right ends of a row the tray holds up (`words.piles`),
/// written in `style` at the two ends of a strip `width` wide whose top left corner is `at`.
pub(super) fn ends(words: &[String; 2], at: Vec2, width: f32, style: TypeStyle) -> Element {
    let word = |words: &str| text(words).style(style).color(color::GOLD);
    let mut ends = row()
        .name("ends")
        .width(width)
        .justify_content(Justify::SpaceBetween);
    ends = ends.child(word(&words[0])).child(word(&words[1]));
    ends.anchor = Some(Anchor::Point(at, Vec2::ZERO));
    ends
}
