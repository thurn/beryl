//! The rollout policy's answers to a catalog question ([`DecisionKind::ChooseObject`] with
//! [`Opt::Number`] options, one per value): the words of a text-changing effect, and a card
//! name. Each other catalog question (a color, a creature type) takes its first value.
//!
//! [`DecisionKind::ChooseObject`]: beryl_engine::DecisionKind::ChooseObject

use beryl_engine::card::{Purpose, Subtypes, TextWord};
use beryl_engine::chosen::{ANOTHER, ChosenValue};
use beryl_engine::two_player::TwoPlayerSeat as _;
use beryl_engine::{Decision, Layout, Opt, SlotId, Zone};

use super::Candidate;
use super::policy::Policy;

/// What a catalog decision's options are scored by, worked out once for the decision.
pub(super) enum Catalog {
    /// The word to replace: a basic land type the object's text names
    /// ([`named_land_types`]).
    ReplaceWord(Subtypes),
    /// The new word: on the player's own objects, the word whose permanents both players
    /// control most, so that a condition such as "unless defending player controls an Island"
    /// or "when you control no Islands" holds; on the opponent's, the word with the fewest
    /// permanents, so that such a condition fails. The counts are [`presence`]'s.
    NewWord {
        ours: bool,
        present: [[i32; 2]; TextWord::COUNT as usize],
    },
    /// The card name ([`card_name`]), by its number.
    CardName(u8),
}

impl Catalog {
    /// What decision `d` is scored by, or `None` when it is not one of the questions this
    /// module answers.
    pub(super) fn of<L: Layout>(p: &Policy<'_, L>, d: &Decision<L>) -> Option<Catalog> {
        Some(match d.prompt.purpose {
            Purpose::ReplaceWord => Catalog::ReplaceWord(named_land_types(p, d)),
            Purpose::NewWord => Catalog::NewWord {
                ours: (L::Slot::all())
                    .filter(|&slot| d.about.contains(slot))
                    .all(|slot| p.obs.controller(slot) == p.me),
                present: presence(p),
            },
            Purpose::CardName => Catalog::CardName(card_name(p)),
            _ => return None,
        })
    }

    /// The score of option `c`.
    pub(super) fn score<L: Layout>(&self, c: Candidate<L>) -> Option<i32> {
        let Some(Opt::Number(n)) = c.option() else {
            return None;
        };
        Some(match *self {
            Catalog::ReplaceWord(named) => match TextWord::of_number(n)? {
                TextWord::LandType(subtype) => i32::from(named.contains(subtype)),
                TextWord::Color(_) => 0,
            },
            Catalog::NewWord { ours, present } => {
                let [mine, theirs] = *present.get(usize::from(n))?;
                if ours {
                    100 * mine.min(theirs) + mine + theirs
                } else {
                    -(mine + theirs)
                }
            }
            Catalog::CardName(name) => i32::from(n == name),
        })
    }
}

/// The basic land types the text of an object `d` is about names: those it has and those a
/// rule of it asks about ([`beryl_engine::card::CardDef::subtypes`]), as "unless defending
/// player controls an Island" names Island.
fn named_land_types<L: Layout>(p: &Policy<'_, L>, d: &Decision<L>) -> Subtypes {
    let mut named = Subtypes::default();
    for slot in L::Slot::all().filter(|&slot| d.about.contains(slot)) {
        if let Some(card) = p.obs.card(slot) {
            p.t.def(card).subtypes(&mut |s| named = named.union(s));
        }
    }
    named
}

/// For each of the ten text words ([`TextWord::of_number`]), how many permanents of that color
/// or basic land type the player controls, then how many the opponent does.
fn presence<L: Layout>(p: &Policy<'_, L>) -> [[i32; 2]; TextWord::COUNT as usize] {
    let mut counts = [[0; 2]; TextWord::COUNT as usize];
    for &slot in p.obs.battlefield() {
        let derived = p.obs.derived(slot);
        let side = usize::from(p.obs.controller(slot) != p.me);
        for (n, count) in (0..TextWord::COUNT).zip(&mut counts) {
            let has = match TextWord::of_number(n) {
                Some(TextWord::Color(color)) => derived.colors.intersects(color),
                Some(TextWord::LandType(subtype)) => {
                    derived.subtypes.contains(p.t.subtypes(), subtype)
                }
                None => false,
            };
            count[side] += i32::from(has);
        }
    }
    counts
}

/// The number of the card name to choose: the card on top of a library when the player knows
/// it, the opponent's library first, else the card the library's deck list has most copies of
/// that the player has not seen leave it. A name no card of the game has ([`ANOTHER`]) is never
/// the answer while one does.
fn card_name<L: Layout>(p: &Policy<'_, L>) -> u8 {
    let obs = p.obs;
    let owners = obs.zone_owners(Zone::Library);
    let opponent = p.me.opponent();
    let owner = if owners.contains(opponent) {
        opponent
    } else {
        owners.sole().unwrap_or(p.me)
    };
    let number = |card: L::Card| ChosenValue::<L>::Name(Some(card)).number();
    for seat in [owner, p.me] {
        if let Some(&top) = obs.library(seat).last()
            && obs.knows(top)
            && let Some(card) = obs.card(top)
        {
            return number(card);
        }
    }
    // A card seen outside the library came from the deck list counted: in a shared-deck game
    // every card did, otherwise the cards the library's owner owns. A deck has few distinct
    // cards, so each is counted by a pass over the deck and the slots, with nothing allocated.
    let seen = |card: L::Card| {
        L::Slot::all()
            .filter(|&slot| {
                let o = obs.object(slot);
                o.zone() != Zone::Library
                    && (obs.shares_zones() || o.owner() == Some(owner))
                    && obs.card(slot) == Some(card)
            })
            .count()
    };
    let deck = || p.t.deck::<L::Card>(owner);
    let mut best = (0, ANOTHER);
    for (i, card) in deck().enumerate() {
        // Each distinct card once, at its first copy.
        if deck().take(i).any(|c| c == card) {
            continue;
        }
        let left = deck()
            .filter(|&c| c == card)
            .count()
            .saturating_sub(seen(card));
        if left > best.0 {
            best = (left, number(card));
        }
    }
    best.1
}
