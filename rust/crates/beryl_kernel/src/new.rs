//! A new game's state: each deck dealt into its slots, and the coin flip (CR 103).

use beryl_card::{TurnStep, Zone};

use crate::bitboard::Bitboard;
use crate::fixed::Fixed;
use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Table};
use crate::object::{Derived, Object};
use crate::pending::Pending;
use crate::player::{ManaPool, MulliganStatus, Player, Turn};
use crate::profile::Profile;
use crate::rng::Rng;
use crate::state::{Budgets, DECK_SLOTS, Flags, State, Zones, deck_slot};
use crate::table::GameTable;

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
        sorceries_as_flash: false,
    }
}

impl State {
    /// A new game between the two decks of `t`, determined entirely by `seed` and the choices
    /// that follow. The first decision is the coin flip winner's choice to play or draw.
    ///
    /// Each deck's cards are dealt into its fixed slots by a permutation drawn from the hidden
    /// shuffle stream (architecture §4.2), so a slot number says nothing about the card in
    /// it. That permutation is also the shuffle of
    /// [CR 103.3](https://yawgatog.com/resources/magic-rules/#R1033): "each player shuffles
    /// their deck so that the cards are in a random order."
    pub fn new(t: &GameTable, seed: u64, profile: Profile) -> State {
        let mut s: State = State::empty(profile);
        s.rng = Rng::new(seed);
        for player in Seat::BOTH {
            let mut cards: Fixed<CardIx, DECK_SLOTS> = Fixed::new();
            for &card in t.deck(player) {
                // GameTable::new refuses decks larger than DECK_SLOTS.
                let _ = cards.push(card);
            }
            s.rng.shuffle.shuffle(cards.as_mut_slice());
            for (i, &card) in cards.iter().enumerate() {
                let slot = deck_slot(player, i);
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
        s
    }
}

impl<L: Layout> State<L> {
    /// A state with no objects, before the game begins.
    pub fn empty(profile: Profile) -> State<L> {
        State {
            objects: Table::filled(Object::EMPTY),
            battlefield: Default::default(),
            derived: Table::filled(Derived::default()),
            listening: 0,
            zones: Zones {
                members: [[Bitboard::EMPTY; Zone::OWNED]; 2],
                stack: Bitboard::EMPTY,
                library: Default::default(),
                graveyard: Default::default(),
                next_timestamp: 0,
            },
            players: [new_player(), new_player()],
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
            effects: Default::default(),
            permissions: Default::default(),
            entered_x: Default::default(),
            pending: Pending::None,
            knowledge: [Bitboard::EMPTY; 2],
            rng: Rng::new(0),
            profile,
            budgets: Budgets::default(),
            flags: Flags::default(),
            outcome: None,
        }
    }
}
