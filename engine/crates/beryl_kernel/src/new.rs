//! A new game's state: each deck dealt into its slots, and the coin flip (see CR 103).
//!
//! Implements: CR 103.3, CR 103.4, CR 728.2, CR 728.5, CR 728.6

use beryl_card::{TurnStep, Zone};

use crate::bitboard::Bitboard;
use crate::chance;
use crate::event::{Listens, Replaces};
use crate::fit::Misfit;
use crate::fixed::Full;
use crate::format::{Format, StartingPlayer, ZoneRule};
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, PileCards, Piles, Queue, Table};
use crate::object::{Derived, Object};
use crate::pending::Pending;
use crate::player::{ManaPool, MulliganStatus, Player};
use crate::profile::Profile;
use crate::rng::Rng;
use crate::rule_changes::Rules;
use crate::state::{Budgets, CounterTable, Effects, Flags, Memos, State, Trackers, Zones};
use crate::static_ability::Inputs;
use crate::store::Store;
use crate::subgame::CompletedTurns;
use crate::table::GameTable;
use crate::turn::{Turn, plan::TurnPlan};

/// A player before the game begins ([CR 103.4]): "Each player begins the game with a starting life
/// total of 20."
///
/// [CR 103.4]: https://yawgatog.com/resources/magic-rules/#R1034
fn new_player<L: Layout>(format: Format) -> Player<L> {
    Player {
        life: L::Life::from(format.starting_life()),
        mana: ManaPool::default(),
        remembered: Default::default(),
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
    /// them. A shared deck, the one library of the shared-deck format, may have
    /// [`SlotId::SHARED_DECK_SLOTS`] cards, all in the shared library.
    ///
    /// Each deck's cards are dealt into its slots by a permutation drawn from the hidden shuffle
    /// stream (architecture §4.2), so a slot number says nothing about the card in it. That
    /// permutation is also the opening shuffle ([CR 103.3]): "each player shuffles their deck so that the
    /// cards are in a random order." A card's place does not depend on the layout, so the same seed
    /// deals the same game in each.
    ///
    /// [CR 103.3]: https://yawgatog.com/resources/magic-rules/#R1033
    pub fn deal(t: &GameTable, seed: u64, profile: Profile) -> Result<State<L>, Misfit> {
        if t.format().zones() == ZoneRule::SharedLibraryAndGraveyard {
            return Self::deal_shared(t, seed, profile);
        }
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
        let format = t.format();
        let mut s: State<L> = State::empty_in(format, profile);
        s.rng = Rng::new(seed);
        for player in Seat::BOTH {
            let mut cards: L::Deck<L::Card> = Default::default();
            for card in t.deck(player) {
                // The deck was just found to fit.
                cards.push_bounded(card);
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
                // A slot for each card of the deck, which was just found to fit.
                s.zones
                    .library
                    .push(player.index(), slot)
                    .expect("a deck that fits");
                s.zones.members[player.index()][Zone::Library.index()].insert(slot);
            }
        }
        s.begin(format);
        Ok(s)
    }

    /// [`State::deal`] for a table of the shared-deck format: its one deck is dealt into the
    /// slots of a shared deck ([`SlotId::shared_deck`]), every card in the shared library and in
    /// both players' sets of it, with no owner ([`Seat::P0`], [`State::has_owner`]). The
    /// permutation that deals them is the one opening shuffle, as for a deck of each player's.
    #[cold]
    fn deal_shared(t: &GameTable, seed: u64, profile: Profile) -> Result<State<L>, Misfit> {
        t.fits::<L>()?;
        let cards = t.deck_len(Seat::P0);
        if cards > L::Slot::SHARED_DECK_SLOTS {
            return Err(Misfit::DeckTooLarge {
                player: Seat::P0,
                cards,
                slots: L::Slot::SHARED_DECK_SLOTS,
            });
        }
        let format = t.format();
        let mut s: State<L> = State::empty_in(format, profile);
        s.rng = Rng::new(seed);
        let mut deck: PileCards<L> = Default::default();
        for card in t.deck(Seat::P0) {
            // The deck was just found to fit.
            deck.push_bounded(card);
        }
        s.rng.shuffle.shuffle(deck.as_mut_slice());
        for (i, &card) in deck.iter().enumerate() {
            let slot = L::Slot::shared_deck(i);
            *s.object_mut(slot) = Object {
                card,
                zone: Zone::Library,
                owner: Seat::P0,
                base_controller: Seat::P0,
                ..Object::EMPTY
            };
            // A slot for each card of the deck, which was just found to fit.
            s.zones
                .library
                .push_shared(slot)
                .expect("a shared deck that fits");
            for members in s.zones.members.as_mut_slice() {
                members[Zone::Library.index()].insert(slot);
            }
        }
        s.begin(format);
        Ok(s)
    }

    /// Opens the game's first question, the coin flip winner's choice to play or draw.
    fn begin(&mut self, format: Format) {
        self.pending = match format.starting_player() {
            StartingPlayer::PlayOrDraw => Pending::PlayOrDraw {
                chooser: chance::starting_player_chooser(self, format),
            },
        };
    }

    /// A fresh game with the current players' physical cards. The marked cards stay in exile
    /// for the effect to put onto the battlefield after mulligans.
    ///
    /// Under [CR 728.2], "All Magic cards involved in the game that was restarted when it
    /// ended, including phased-out permanents and nontraditional Magic cards, are involved
    /// in the new game". This includes cards admitted from a sideboard. Under [CR 728.5],
    /// "These cards are not in their owner’s deck as the new game begins" if exempted.
    /// Under [CR 728.6], "If a Magic subgame (see rule 730) is restarted, the main game
    /// is unaffected."
    ///
    /// [CR 728.2]: https://yawgatog.com/resources/magic-rules/#R7282
    /// [CR 728.5]: https://yawgatog.com/resources/magic-rules/#R7285
    /// A restarted game of the shared-deck format is one too: every card that returns goes to
    /// the shared library once, with no owner, and the library is shuffled once.
    ///
    /// [CR 728.6]: https://yawgatog.com/resources/magic-rules/#R7286
    #[cold]
    pub fn restart(
        &mut self,
        table: &GameTable,
        starter: Seat,
        carried: Bitboard<L::Slot>,
    ) -> Result<(), Full> {
        let mut next = Self::empty_in(table.format(), self.profile);
        next.set_starting_player(starter);
        next.rng = self.rng;
        next.sideboard_taken = self.sideboard_taken;
        let shared = next.shares_zones();
        for player in self.players().in_seat_order() {
            for slot in self.in_game_card_slots(table, player) {
                let previous = self.object(slot);
                if previous.zone == Zone::Nowhere || previous.is_token() {
                    continue;
                }
                let kept_exiled = previous.zone == Zone::Exile && carried.contains(slot);
                // In the shared-deck format a card in exile keeps the owner it had there, and
                // a card in the shared library has none.
                let (zone, owner) = match (kept_exiled, shared) {
                    (true, true) => (Zone::Exile, previous.owner),
                    (true, false) => (Zone::Exile, player),
                    (false, true) => (Zone::Library, Seat::P0),
                    (false, false) => (Zone::Library, player),
                };
                *next.object_mut(slot) = Object {
                    card: self.card_really_is(slot),
                    zone,
                    owner,
                    base_controller: owner,
                    ..Object::EMPTY
                };
                if kept_exiled {
                    next.zones.members[owner.index()][zone.index()].insert(slot);
                    for known in next.knowledge.as_mut_slice() {
                        known.insert(slot);
                    }
                } else if shared {
                    next.zones.library.push_shared(slot)?;
                    for members in next.zones.members.as_mut_slice() {
                        members[zone.index()].insert(slot);
                    }
                } else {
                    next.zones.members[owner.index()][zone.index()].insert(slot);
                    next.zones.library.push(owner.index(), slot)?;
                }
            }
        }
        for player in next.zone_owners(Zone::Library).in_seat_order() {
            let list = next.pile_of(player);
            let objects = &next.objects;
            next.zones
                .library
                .list_mut(list)
                .sort_unstable_by_key(|slot| objects[slot.index()].card);
            next.shuffle_library(player)?;
        }
        // Turn counts saturate at `u16::MAX` rather than wrapping elapsed turns.
        next.restarted_turns = self
            .restarted_turns
            .saturating_add(self.turn.number())
            .saturating_add(self.subgame_turns.get());
        next.outer = std::mem::take(&mut self.outer);
        *self = next;
        Ok(())
    }

    /// A state with no objects, before the game begins.
    pub fn empty(profile: Profile) -> State<L> {
        Self::empty_in(Format::CONSTRUCTED, profile)
    }

    pub(crate) fn empty_in(format: Format, profile: Profile) -> State<L> {
        State {
            objects: Table::filled(Object::EMPTY),
            battlefield: Default::default(),
            derived: Table::filled(Derived::default()),
            listening: Listens::NONE,
            replacing: Replaces::NONE,
            layer_inputs: Inputs::NONE,
            static_rules: Table::filled(Rules::NONE),
            zones: Zones {
                members: Table::filled([Bitboard::EMPTY; Zone::OWNED]),
                stack: Bitboard::EMPTY,
                library: Default::default(),
                graveyard: Default::default(),
                next_timestamp: 0,
                mask: u8::from(format.zones() == ZoneRule::PerPlayer),
            },
            players: Table::from_fn(|_| new_player(format)),
            turn: Turn {
                number: 0,
                active: Seat::P0,
                starting_player: Seat::P0,
                step: TurnStep::Untap,
                priority: Seat::P0,
                passes: 0,
                cleanup_check: false,
                attacked: false,
                controller: None,
                after_extra: None,
                starting_draw_later: false,
            },
            stack: Default::default(),
            captured_words: Default::default(),
            triggers: Default::default(),
            delayed: Default::default(),
            last_known: Default::default(),
            departed_spells: Default::default(),
            effects: Effects::empty(),
            keyed: Bitboard::EMPTY,
            phased_out: Bitboard::EMPTY,
            memos: Memos::empty(),
            counters: CounterTable::empty(),
            elsewhere: Default::default(),
            grants: Default::default(),
            sideboard_taken: crate::sideboard::Taken::default(),
            turn_plan: TurnPlan::default(),
            trackers: Trackers::empty(),
            pending: Pending::None,
            knowledge: Table::filled(Bitboard::EMPTY),
            rng: Rng::new(0),
            repeats: crate::repeats::Repeats::default(),
            // A new game has matched no state trigger yet.
            states_due: true,
            profile,
            budgets: Budgets::default(),
            flags: Flags::default(),
            outcome: None,
            restarted_turns: 0,
            outer: Default::default(),
            subgame_turns: Default::default(),
        }
    }
}
