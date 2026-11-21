//! Battlefield grouping and the mapping between permanents and ordered row items.

use std::f32::consts::FRAC_1_SQRT_2;

use beryl_protocol::protocol::{CardType, FaceName, Facing, ObjectId, ObjectView, ZoneName};
use glam::{Quat, Vec3};
use kit_space::arrange::Edge;
use kit_space::arrange::row::{Group, Member, Placed, Tuck};

use super::place::host_slot;
use super::{CARD_HEIGHT, CARD_WIDTH, LAYER, Layout};
use kit_base::data::space::card::PHASED_OUT_OPACITY;
use kit_base::pose::Pose;

mod change;
mod names;
pub use change::RowChange;
pub use names::{NameIds, SameName};

/// Which of a row's groups are in view: `shown` of its `groups` from `first`, with `before`
/// cards scrolled off its left end and `after` off its right end. A row that fits shows every
/// group from the first. Its numbers are kept in `u16`s, which hold any row of a table, because a
/// window for each row is kept in every script's view at rest: as `usize`s they would make each
/// waiting script larger than 1024 bytes, past which a growing `Vec` of them reallocates more
/// often.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RowWindow {
    groups: u16,
    first: u16,
    shown: u16,
    before: u16,
    after: u16,
}

impl RowWindow {
    /// A window of `shown` of `groups` groups from `first`, with `before` and `after` cards off
    /// the two ends.
    pub(super) fn new(
        groups: usize,
        first: usize,
        shown: usize,
        (before, after): (usize, usize),
    ) -> Self {
        let small = |number: usize| u16::try_from(number).unwrap_or(u16::MAX);
        Self {
            groups: small(groups),
            first: small(first),
            shown: small(shown),
            before: small(before),
            after: small(after),
        }
    }

    /// The first group in view.
    #[must_use]
    pub fn first(&self) -> usize {
        usize::from(self.first)
    }

    /// How many groups the row holds.
    #[must_use]
    pub fn groups(&self) -> usize {
        usize::from(self.groups)
    }

    /// How many groups are in view.
    #[must_use]
    pub fn shown(&self) -> usize {
        usize::from(self.shown)
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

    /// Whether the row has cards off either end, and so scrolls.
    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.before + self.after > 0
    }

    /// The first group it may show: from 0 to the last from which every group to the end fits.
    #[must_use]
    pub fn clamp(&self, first: isize) -> usize {
        let last = usize::from(self.groups.saturating_sub(self.shown));
        usize::try_from(first.max(0)).unwrap_or(0).min(last)
    }
}

/// The cards that lie under a row's permanents (`place::under`), each with the permanent it
/// lies under: the other cards of a merged permanent and the cards a permanent holds in exile,
/// which peek out to its left, and the Auras and Equipment attached to it, which peek out to
/// its right, as the web interface's `tuck` draws them.
pub struct Beneath<'a> {
    cards: Vec<(ObjectId, &'a ObjectView)>,
}

impl<'a> Beneath<'a> {
    /// The cards of `under` that lie under one of `permanents`, each with the permanent it lies
    /// under.
    pub fn new(under: Vec<&'a ObjectView>, permanents: &[&'a ObjectView]) -> Self {
        let find = |slot: ObjectId| permanents.iter().copied().find(|p| p.slot == slot);
        let cards = under
            .into_iter()
            .filter_map(|card| Some((host_slot(card, find)?, card)))
            .collect();
        Self { cards }
    }

    /// The cards under the permanent in `host` that peek out to its left (`left`) or to its
    /// right, in the order the row lists them.
    pub fn of(&self, host: ObjectId, left: bool) -> impl Iterator<Item = &'a ObjectView> + '_ {
        self.cards
            .iter()
            .filter(move |(holder, card)| *holder == host && peeks_left(card) == left)
            .map(|(_, card)| *card)
    }

    /// Whether any card lies under the permanent in `host`.
    fn holds(&self, host: ObjectId) -> bool {
        self.cards.iter().any(|(holder, _)| *holder == host)
    }
}

/// Whether a card under a permanent peeks out to its left: every card but an Aura or Equipment
/// attached to it, which peeks out to its right, as the web interface's `tuck` has it.
fn peeks_left(card: &ObjectView) -> bool {
    card.zone != ZoneName::Battlefield
}

/// A row's cards gathered into groups laid out side by side.
pub enum Groups<'a, 'b> {
    /// Each card a group of its own.
    Single(&'b [&'a ObjectView]),
    /// Each group's cards a run of `cards`, its top card first; `ends` holds where each run
    /// ends.
    Piles {
        cards: Vec<&'a ObjectView>,
        ends: Vec<usize>,
    },
}

impl<'a> Groups<'a, '_> {
    fn len(&self) -> usize {
        match self {
            Groups::Single(cards) => cards.len(),
            Groups::Piles { ends, .. } => ends.len(),
        }
    }

    fn get(&self, index: usize) -> &[&'a ObjectView] {
        match self {
            Groups::Single(cards) => &cards[index..=index],
            Groups::Piles { cards, ends } => {
                let start = index.checked_sub(1).map_or(0, |before| ends[before]);
                &cards[start..ends[index]]
            }
        }
    }
    /// The groups, in order.
    pub fn piles(&self) -> impl Iterator<Item = &[&'a ObjectView]> {
        (0..self.len()).map(|index| self.get(index))
    }
}

/// `cards` gathered into the groups the solver lays out side by side: identical permanents into
/// piles where the row piles them (`stack_slots`), and each other card alone.
pub fn grouped<'a, 'b>(
    (cards, beneath): (&'b [&'a ObjectView], &Beneath<'a>),
    (names, ids, stack_slots): (Option<SameName>, Option<&NameIds<'_>>, Option<&[ObjectId]>),
) -> Groups<'a, 'b> {
    stack_slots
        .filter(|_| cards.len() > 1)
        .map_or(Groups::Single(cards), |stack_slots| {
            // On a short row, rule out singletons before allocating the name index.
            if names.is_none()
                && cards.len() <= 8
                && !cards
                    .iter()
                    .enumerate()
                    .any(|(index, &card)| cards[..index].iter().any(|other| twins(card, other)))
            {
                return Groups::Single(cards);
            }
            piles(
                cards,
                &names.unwrap_or_else(|| SameName::of(cards, ids)),
                beneath,
                stack_slots,
            )
        })
}

impl Groups<'_, '_> {
    /// Describe the row's groups without giving the solver any game objects.
    pub(super) fn solver_groups(
        &self,
        beneath: &Beneath<'_>,
        layout: &Layout,
        faces: &[(ObjectId, f32)],
    ) -> impl Iterator<Item = Group> {
        self.piles().map(move |pile| {
            let face = pile[0];
            let tucks = beneath
                .cards
                .iter()
                .enumerate()
                .filter(|(_, (host, _))| *host == face.slot)
                .map(|(key, (_, card))| Tuck {
                    key,
                    width: card_width(card),
                    edge: if peeks_left(card) {
                        Edge::Start
                    } else {
                        Edge::End
                    },
                })
                .collect();
            Group {
                width: card_width(face),
                copies: pile.len(),
                gap_class: face.tapped,
                anchor: faces
                    .iter()
                    .find_map(|&(key, x)| (key == face.slot).then_some(x)),
                tucks,
                // Keeping tucks level on compact rows keeps them clear of adjacent rows.
                tuck_rise: if face.types.contains(&CardType::Land) || layout.phone.is_some() {
                    0.0
                } else {
                    layout.beneath_step.y * CARD_HEIGHT
                },
            }
        })
    }
}

impl<'a> Groups<'a, '_> {
    /// Turn an arranged item's centre and layer into its card pose.
    pub(super) fn pose(
        &self,
        beneath: &Beneath<'a>,
        placed: &Placed,
        y: f32,
    ) -> (&'a ObjectView, Pose) {
        let (object, rotation) = match placed.member {
            Member::Copy(index) => {
                let object = self.get(placed.group)[index];
                let rotation = if object.tapped {
                    Quat::from_xyzw(0.0, 0.0, -FRAC_1_SQRT_2, FRAC_1_SQRT_2)
                } else {
                    Quat::IDENTITY
                };
                (object, rotation)
            }
            Member::Tuck(index) => (beneath.cards[index].1, Quat::IDENTITY),
        };
        let pose = Pose {
            position: Vec3::new(
                placed.centre.x,
                y + placed.centre.y,
                LAYER * placed.layer as f32,
            ),
            rotation,
            ..Pose::default()
        };
        let pose = match placed.member {
            Member::Copy(_) => reposed(object, pose),
            Member::Tuck(_) => Pose {
                turn: if face_up(object) { 0.0 } else { 1.0 },
                ..pose
            },
        };
        (object, pose)
    }
}

fn card_width(object: &ObjectView) -> f32 {
    if object.tapped {
        CARD_HEIGHT
    } else {
        CARD_WIDTH
    }
}

/// `pose` with what `object`'s own record decides of a permanent's pose in its row and nothing
/// else does: whether it shows its face, and how faded it is. A step that changes only those
/// (`RowChange::Pose`) gives the card its new pose with this, and lays out nothing else.
pub fn reposed(object: &ObjectView, pose: Pose) -> Pose {
    Pose {
        turn: if face_up(object) { 0.0 } else { 1.0 },
        // A phased-out permanent stays where it lay, faded, as the web interface draws it:
        // it is treated as though it does not exist
        // ([CR 702.26b](https://yawgatog.com/resources/magic-rules/#R70226b)).
        opacity: if object.phased_out {
            PHASED_OUT_OPACITY
        } else {
            1.0
        },
        ..pose
    }
}

/// Whether the viewer sees a card's face: a face-up card whose name they know.
pub(super) fn face_up(object: &ObjectView) -> bool {
    object.facing == Facing::Up && object.name.is_some()
}

/// `cards` with identical permanents gathered into piles, each pile where its first card was.
/// The pile's first card is the one with the lowest slot, which lies on top: the engine offers
/// the lowest slot of interchangeable objects, so what a choice takes leaves from the top.
///
/// Only cards with the same name can be twins, so each card is compared only with the piles of
/// its own name, found through `names`. A row of a thousand different names would otherwise
/// compare every card with every pile.
fn piles<'a, 'b>(
    cards: &'b [&'a ObjectView],
    names: &SameName,
    beneath: &Beneath,
    stack_slots: &[ObjectId],
) -> Groups<'a, 'b> {
    if !names.repeats() {
        return Groups::Single(cards);
    }
    // The index in `cards` of the first card of each card's pile.
    let mut tops: Vec<usize> = (0..cards.len()).collect();
    let plain = |card: &ObjectView| {
        plain(card) && !beneath.holds(card.slot) && stack_slots.binary_search(&card.slot).is_err()
    };
    for (index, &card) in cards.iter().enumerate() {
        if !plain(card) {
            continue;
        }
        // Twins is an equivalence: a card that twins any card of a pile twins its first card,
        // and of the piles of one name at most one takes it.
        let mut earlier = names.before(index);
        while let Some(other) = earlier {
            if plain(cards[other]) && twins(cards[other], card) {
                tops[index] = tops[other];
                break;
            }
            earlier = names.before(other);
        }
    }
    if tops.iter().enumerate().all(|(index, &top)| index == top) {
        return Groups::Single(cards);
    }
    // Each pile's cards as one run, the piles in the order of their first cards: count each
    // pile's cards, then put each card at its pile's next free place.
    let mut ends: Vec<usize> = Vec::with_capacity(cards.len());
    let mut pile_of = vec![0; cards.len()];
    for index in 0..cards.len() {
        if tops[index] == index {
            pile_of[index] = ends.len();
            ends.push(0);
        }
        ends[pile_of[tops[index]]] += 1;
    }
    let mut total = 0;
    for end in &mut ends {
        total += *end;
        *end = total;
    }
    let mut next: Vec<usize> = std::iter::once(0).chain(ends.iter().copied()).collect();
    let mut gathered = cards.to_vec();
    for (index, &card) in cards.iter().enumerate() {
        let pile = pile_of[tops[index]];
        gathered[next[pile]] = card;
        next[pile] += 1;
    }
    let mut start = 0;
    for &end in &ends {
        if end - start > 1 {
            gathered[start..end].sort_by_key(|object| object.slot.0);
        }
        start = end;
    }
    Groups::Piles {
        cards: gathered,
        ends,
    }
}

/// Whether a permanent may share a pile: nothing on it or about it that a pile would hide.
pub(super) fn plain(object: &ObjectView) -> bool {
    face_up(object)
        && object.face != FaceName::Down
        && object.note.as_deref().is_none_or(str::is_empty)
        && !object.doesnt_untap
        && object.named_counters.is_empty()
        && object.counters.is_empty()
        && object.damage == 0
        && object.combat.is_none()
        && object.attached_to.is_none()
        && !object.phased_out
        && object.really.as_deref().is_none_or(str::is_empty)
        && object.chosen.as_deref().is_none_or(str::is_empty)
}

/// Whether two permanents are alike in everything the table shows of them.
pub(super) fn twins(a: &ObjectView, b: &ObjectView) -> bool {
    a.slot != b.slot && a.name.is_some() && same_pile_face(a, b)
}

fn same_pile_face(a: &ObjectView, b: &ObjectView) -> bool {
    a.name == b.name
        && a.facing == b.facing
        && a.zone == b.zone
        && a.owner == b.owner
        && a.controller == b.controller
        && a.token == b.token
        && a.tapped == b.tapped
        && a.summoning_sick == b.summoning_sick
        && a.types == b.types
        && a.pt == b.pt
}
