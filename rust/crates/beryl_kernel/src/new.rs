//! A new game's state: each deck dealt into its slots, and the coin flip (CR 103).

use beryl_card::{Rules, TurnStep, Zone};

use crate::bitboard::Bitboard;
use crate::fit::Misfit;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Table};
use crate::object::{Derived, Object};
use crate::pending::Pending;
use crate::player::{ManaPool, MulliganStatus, Player};
use crate::profile::Profile;
use crate::rng::Rng;
use crate::state::{Budgets, Effects, Flags, State, Zones};
use crate::store::Store;
use crate::table::GameTable;
use crate::turn::Turn;

/// A player before the game begins ([CR 103.4](https://yawgatog.com/resources/magic-rules/#R1034)):
/// "Each player begins the game with a starting life total of 20."
fn new_player<L: Layout>() -> Player<L> {
    Player {
        life: 20,
        mana: ManaPool::default(),
        lands_played: 0,
        drew_from_empty: false,
        mulligans: 0,
        mulligan: MulliganStatus::Undeclared,
        to_bottom: 0,
        emblems: Default::default(),
    }
}

impl State {
    /// A new game between the decks of `t` in [`crate::Small`], the layout search runs on
    /// ([`State::deal`]).
    pub fn new(t: &GameTable, seed: u64, profile: Profile) -> Result<State, Misfit> {
        State::deal(t, seed, profile)
    }
}

impl<L: Layout> State<L> {
    /// A new game between the decks of `t`, determined entirely by `seed` and the choices
    /// that follow. The first decision is the coin flip winner's choice to play or draw. A
    /// deck with more cards than the layout has slots for one is refused
    /// ([`Misfit::DeckTooLarge`]), and so is a table of more distinct cards than the layout's
    /// game index numbers ([`Misfit::TooManyCards`]): the game is dealt in a layout that has
    /// them.
    ///
    /// Each deck's cards are dealt into its slots by a permutation drawn from the hidden
    /// shuffle stream (architecture §4.2), so a slot number says nothing about the card in
    /// it. That permutation is also the shuffle of
    /// [CR 103.3](https://yawgatog.com/resources/magic-rules/#R1033): "each player shuffles
    /// their deck so that the cards are in a random order." A card's place does not depend
    /// on the layout, so the same seed deals the same game in each.
    pub fn deal(t: &GameTable, seed: u64, profile: Profile) -> Result<State<L>, Misfit> {
        t.fits::<L>()?;
        for player in Seat::BOTH {
            let cards = t.deck_len(player);
            if cards > L::Slot::DECK_SLOTS {
                return Err(Misfit::DeckTooLarge {
                    player,
                    cards,
                    slots: L::Slot::DECK_SLOTS,
                });
            }
        }
        let mut s: State<L> = State::empty(profile);
        s.rng = Rng::new(seed);
        for player in Seat::BOTH {
            let mut cards: L::Deck<L::Card> = Default::default();
            for card in t.deck(player) {
                // The deck was just found to fit.
                let _ = cards.push(card);
            }
            s.rng.shuffle.shuffle(cards.as_mut_slice());
            for (i, &card) in cards.iter().enumerate() {
                let slot = L::Slot::deck(player, i);
                *s.object_mut(slot) = Object {
                    card,
                    zone: Zone::Library,
                    owner: player,
                    base_controller: player,
                    ..Object::EMPTY
                };
                let _ = s.zones.library[player.index()].push(slot);
                s.zones.members[player.index()][Zone::Library as usize].insert(slot);
            }
        }
        // CR 103.1: "the players determine which one of them will choose who takes the first
        // turn ... (flipping a coin, rolling dice, etc.)".
        let chooser = Seat::new(s.rng.other.below(2) as u8);
        s.pending = Pending::PlayOrDraw { chooser };
        Ok(s)
    }

    /// A state with no objects, before the game begins.
    pub fn empty(profile: Profile) -> State<L> {
        State {
            objects: Table::filled(Object::EMPTY),
            battlefield: Default::default(),
            derived: Table::filled(Derived::default()),
            listening: 0,
            static_rules: Table::filled(Rules::NONE),
            zones: Zones {
                members: Table::filled([Bitboard::EMPTY; Zone::OWNED]),
                stack: Bitboard::EMPTY,
                library: Table::filled(Default::default()),
                graveyard: Table::filled(Default::default()),
                next_timestamp: 0,
            },
            players: Table::from_fn(|_| new_player()),
            turn: Turn {
                number: 0,
                active: Seat::P0,
                starting_player: Seat::P0,
                step: TurnStep::Untap,
                priority: Seat::P0,
                passes: 0,
                cleanup_check: false,
                attacked: false,
            },
            stack: Default::default(),
            triggers: Default::default(),
            delayed: Default::default(),
            effects: Effects::empty(),
            keyed: Bitboard::EMPTY,
            entered_x: Default::default(),
            pending: Pending::None,
            knowledge: Table::filled(Bitboard::EMPTY),
            rng: Rng::new(0),
            profile,
            budgets: Budgets::default(),
            flags: Flags::default(),
            outcome: None,
        }
    }
}
