//! What chance produced, as a save records it (architecture §14): the identity of each hidden
//! card at the moment a player first learns it, and each coin flip's result. A replay
//! reproduces these outcomes rather than the random draws behind them, so it does not depend
//! on the seed, on how the streams are derived, or on how many draws the engine makes.
//!
//! The session lends a [`Chance`] to the engine through the sink of the calls that play the
//! game on ([`crate::event::Sink::set_chance`]). Search never does, so its copies of the state
//! pay one branch at each chokepoint and record nothing.
//!
//! A card's identity matters from the moment someone learns it: until then, which card sits
//! in a slot no player knows could be any card of the rest of its owner's decklist, as a
//! determinization deals it ([`crate::Observation::determinize`]). No one may look at a
//! library ([CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players can't
//! look at or change the order of cards in a library"), and a shuffle leaves its cards in an
//! order no player knows
//! ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "randomize the cards
//! within it so that no player knows their order"). So replay binds a recorded
//! card to the slot being revealed by swapping identities with another slot of the same owner
//! and zone that no player knows, the move a shuffle's rebinding makes ([`crate::zone`]). A
//! shuffled library's order never matters until one of its cards is revealed.

use std::collections::VecDeque;

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::ids::{CardIx, Seat};
use crate::layout::Layout;
use crate::state::{ObjectFlags, State, TurnStep, Zone};

/// The outcome of one random event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Random {
    /// A card no player knew became known: `card` is what `owner`'s slot holds.
    Reveal { owner: Seat, card: CardIx },
    /// `player` flipped a coin and won the flip, or lost it.
    Flip { player: Seat, won: bool },
}

/// Where a replay could not reproduce what the record says chance produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub turn: u16,
    pub step: TurnStep,
    /// What the record holds next of this kind, if anything.
    pub recorded: Option<Random>,
    /// What happened instead: for a reveal, the slot's owner and the card the game's own
    /// randomness had put there.
    pub happened: Random,
}

/// The outcomes of a game's random events: recorded as they happen, and while replaying,
/// dictated by a record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chance {
    /// Every outcome since the session last took them ([`Chance::take`]), in order.
    happened: Vec<Random>,
    /// While replaying, the recorded outcomes still to come, each kind in its own order: a
    /// later build that reveals a card at another moment relative to a flip still replays.
    replaying: Option<Recorded>,
    /// The first outcome replay could not reproduce.
    mismatch: Option<Mismatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Recorded {
    reveals: VecDeque<(Seat, CardIx)>,
    flips: VecDeque<(Seat, bool)>,
}

impl Chance {
    /// Has the outcomes in `record` happen, each kind in order, as chance would have. A
    /// recorded outcome is consumed whether or not it could be reproduced.
    pub fn replay(&mut self, record: impl IntoIterator<Item = Random>) {
        let r = self.replaying.get_or_insert_with(Recorded::default);
        for outcome in record {
            match outcome {
                Random::Reveal { owner, card } => r.reveals.push_back((owner, card)),
                Random::Flip { player, won } => r.flips.push_back((player, won)),
            }
        }
    }

    /// Lets chance take its course again: the game's own randomness decides from here.
    pub fn stop_replaying(&mut self) {
        self.replaying = None;
    }

    /// The first recorded outcome still to come while replaying, if any: a reveal before a
    /// flip.
    pub fn next_recorded(&self) -> Option<Random> {
        let r = self.replaying.as_ref()?;
        r.reveals
            .front()
            .map(|&(owner, card)| Random::Reveal { owner, card })
            .or_else(|| {
                r.flips
                    .front()
                    .map(|&(player, won)| Random::Flip { player, won })
            })
    }

    /// The outcomes since the last take, in order.
    pub fn take(&mut self) -> Vec<Random> {
        std::mem::take(&mut self.happened)
    }

    /// The first outcome replay could not reproduce, if any, which it forgets.
    pub fn take_mismatch(&mut self) -> Option<Mismatch> {
        self.mismatch.take()
    }

    fn replaying(&self) -> bool {
        self.replaying.is_some()
    }

    fn mismatch<L: Layout>(&mut self, s: &State<L>, recorded: Option<Random>, happened: Random) {
        self.mismatch.get_or_insert(Mismatch {
            turn: s.turn.number,
            step: s.turn.step,
            recorded,
            happened,
        });
    }
}

/// Whether no player knows the card in `slot`.
fn unknown<L: Layout>(s: &State<L>, slot: L::Slot) -> bool {
    !s.knowledge[0].contains(slot) && !s.knowledge[1].contains(slot)
}

/// Settles the identity of the card in `slot` as a player is about to learn it, at a
/// knowledge chokepoint (architecture §13): draw, look at, reveal, and any move to a public
/// zone or a hand. Only a card no player knows yet is settled, and a token never is: what it
/// is was never hidden.
#[inline]
pub fn settle<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, slot: L::Slot) {
    settle_but(s, sink, slot, Bitboard::EMPTY);
}

/// [`settle`], trading identities with none of `settled`: cards settled just before, which a
/// player is about to learn with this one.
fn settle_but<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    slot: L::Slot,
    settled: Bitboard<L::Slot>,
) {
    let Some(chance) = sink.chance.as_deref_mut() else {
        return;
    };
    let o = *s.object(slot);
    if !unknown(s, slot) || o.flags.has(ObjectFlags::TOKEN) {
        return;
    }
    // A paused body run again reveals what its earlier runs revealed: chance took its course
    // there, once (crate::resolve).
    if sink.revealed.contains(slot) {
        sink.revealed.remove(slot);
        return;
    }
    if let Some(r) = &mut chance.replaying {
        let happened = Random::Reveal {
            owner: o.owner,
            card: o.card,
        };
        match r.reveals.pop_front() {
            None => chance.mismatch(s, None, happened),
            Some((owner, card)) => {
                let recorded = Some(Random::Reveal { owner, card });
                if owner != o.owner {
                    chance.mismatch(s, recorded, happened);
                } else if card != o.card {
                    match partner(s, slot, card, settled | sink.revealed) {
                        Some(p) => {
                            s.object_mut(p).card = o.card;
                            s.object_mut(slot).card = card;
                        }
                        None => chance.mismatch(s, recorded, happened),
                    }
                }
            }
        }
    }
    chance.happened.push(Random::Reveal {
        owner: o.owner,
        card: s.object(slot).card,
    });
}

/// Takes from `run`, a copy of `s` played on, the identity of each library card no player
/// knows in `s`: a run that replayed a record may have bound recorded cards to the slots it
/// revealed, trading identities among the cards no one knew ([`settle`]). They stay unknown
/// to every player in `s`.
pub fn adopt_unknown<L: Layout>(s: &mut State<L>, run: &State<L>) {
    let known = s.knowledge[0] | s.knowledge[1];
    for p in Seat::BOTH {
        for slot in (s.zone(p, Zone::Library) & !known).iter() {
            s.object_mut(slot).card = run.object(slot).card;
        }
    }
}

/// Another slot of the owner of `slot`, in its zone, that no player knows and that holds
/// `card`, but those `settled` (or revealed by an earlier run of a paused body, whose
/// identities are known in the committed state): the first by slot, as any of them would do.
fn partner<L: Layout>(
    s: &State<L>,
    slot: L::Slot,
    card: CardIx,
    settled: Bitboard<L::Slot>,
) -> Option<L::Slot> {
    let o = s.object(slot);
    if o.zone as usize >= Zone::OWNED {
        return None;
    }
    (s.zone(o.owner, o.zone) & !settled)
        .iter()
        .find(|&p| p != slot && unknown(s, p) && s.object(p).card == card)
}

/// [`settle`] for each card in `cards`, those in a library from its top down and then the
/// rest by slot: an order the game's randomness does not choose, as it chooses which slots
/// are where in a library.
pub fn settle_all<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, cards: Bitboard<L::Slot>) {
    if sink.chance.is_none() {
        return;
    }
    let mut settled = Bitboard::EMPTY;
    for owner in Seat::BOTH {
        for i in (0..s.library(owner).len()).rev() {
            let slot = s.library(owner)[i];
            if cards.contains(slot) {
                settle_but(s, sink, slot, settled);
                settled.insert(slot);
            }
        }
    }
    for slot in (cards & !settled).iter() {
        settle_but(s, sink, slot, settled);
        settled.insert(slot);
    }
}

/// Flips a coin for `player` and returns whether they won the flip: one draw from the game's
/// randomness, or while replaying, the recorded result.
pub fn flip<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, player: Seat) -> bool {
    let drawn = s.rng.other.below(2) == 0;
    let Some(chance) = sink.chance.as_deref_mut() else {
        return drawn;
    };
    let mut won = drawn;
    if let Some(r) = &mut chance.replaying {
        let happened = Random::Flip { player, won };
        match r.flips.pop_front() {
            Some((p, recorded)) if p == player => won = recorded,
            recorded => chance.mismatch(
                s,
                recorded.map(|(player, won)| Random::Flip { player, won }),
                happened,
            ),
        }
    }
    chance.happened.push(Random::Flip { player, won });
    won
}

/// Whether the sink holds a chance that is replaying a record, which may have bound
/// identities.
pub fn replaying<L: Layout>(sink: &Sink<L>) -> bool {
    sink.chance.as_deref().is_some_and(Chance::replaying)
}
