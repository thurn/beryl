//! Whether a game fits a [`Layout`]: its decks the slots a player's deck is given, its tokens
//! the pool, each of its lists the room the layout gives that list. A game that does not is
//! refused where a state of the layout is made, with a [`Misfit`] and never a panic
//! ([`State::deal`], [`State::try_relayout`]).
//!
//! Implements: none. Checks whether game data fits a storage layout.

use std::fmt;

use crate::decision::{Candidates, Opt};
use crate::ids::{CardId, Seat, SlotId, SlotPlace};
use crate::layout::{Item, Layout, Number, Queue, Table};
use crate::object::{Derived, Object};
use crate::pending::{Pending, Resolving, hidden_pick_of};
use crate::player::Player;
use crate::relayout::Relayout;
use crate::stack::{SpentMana, StackEntry};
use crate::state::State;
use crate::subgame::{CompletedTurns, OuterGame};
use crate::table::GameTable;

/// Why a game does not fit a layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Misfit {
    /// A deck has more cards than the layout has slots for one player's
    /// ([`SlotId::DECK_SLOTS`]).
    DeckTooLarge {
        player: Seat,
        cards: usize,
        slots: usize,
    },
    /// A token or spell copy sits past the layout's pool: the game has had more of them at
    /// once than the pool's `slots`.
    PoolTooSmall { slots: usize },
    /// One of the game's lists is longer than the layout lets it grow.
    ListTooLong {
        list: &'static str,
        len: usize,
        capacity: usize,
    },
    /// The game's table holds more distinct cards, tokens and emblems than the layout's game
    /// index numbers ([`CardId::COUNT`] of its [`Layout::Card`]).
    TooManyCards { cards: usize, indices: usize },
    /// The layout cannot represent a recorded answer or option, or has no matching slot
    /// for a hidden pick.
    IndexTooWide { index: usize },
    /// An active subgame or its completed turns require Large.
    Subgame,
    /// An ability on the stack remembers the mana spent on its X, which only Large holds
    /// ([`crate::state::SpentMana`]).
    SpentOnX,
    /// A numeric quantity (power, toughness, counters, damage, mana, life or a tracker count)
    /// is outside what the layout's numeric type can represent.
    ValueOutOfRange {
        quantity: &'static str,
        value: i64,
        min: i64,
        max: i64,
    },
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Misfit::Subgame => f.write_str("subgames require the Large layout"),
            Misfit::SpentOnX => f.write_str("mana spent on X requires the Large layout"),
            Misfit::DeckTooLarge {
                player,
                cards,
                slots,
            } => write!(
                f,
                "player {}'s deck has {cards} cards, more than the {slots} this layout holds",
                player.get()
            ),
            Misfit::PoolTooSmall { slots } => write!(
                f,
                "more tokens and spell copies than the {slots} this layout holds"
            ),
            Misfit::ListTooLong {
                list,
                len,
                capacity,
            } => write!(
                f,
                "{list} holds {len}, more than the {capacity} this layout lets it"
            ),
            Misfit::TooManyCards { cards, indices } => write!(
                f,
                "the game has {cards} distinct cards, more than the {indices} this layout numbers"
            ),
            Misfit::IndexTooWide { index } => {
                write!(f, "option {index} is past what this layout can name")
            }
            Misfit::ValueOutOfRange {
                quantity,
                value,
                min,
                max,
            } => write!(
                f,
                "{quantity} is {value}, outside the {min}..={max} this layout holds"
            ),
        }
    }
}

impl std::error::Error for Misfit {}

/// The room the queue `field` reads from a `V` has: its type's, so no `V` is made to ask.
fn capacity<V, T: Item, Q: Queue<T>>(_field: impl Fn(&V) -> &Q) -> usize {
    Q::CAPACITY
}

/// Whether a list of `len` fits `capacity`.
pub(crate) fn room(list: &'static str, len: usize, capacity: usize) -> Result<(), Misfit> {
    if len <= capacity {
        Ok(())
    } else {
        Err(Misfit::ListTooLong {
            list,
            len,
            capacity,
        })
    }
}

/// Whether `value` fits in the numeric type `N`.
pub(crate) fn fits_number<N: Number>(quantity: &'static str, value: i64) -> Result<(), Misfit> {
    if N::try_from_i64(value).is_some() {
        Ok(())
    } else {
        Err(Misfit::ValueOutOfRange {
            quantity,
            value,
            min: N::MIN.to_i64(),
            max: N::MAX.to_i64(),
        })
    }
}

/// Whether `derived`'s power and toughness fit layout `M`.
fn fits_derived<L: Layout, M: Layout>(derived: &Derived<L>) -> Result<(), Misfit> {
    fits_number::<M::Pt>("power", derived.power.to_i64())?;
    fits_number::<M::Pt>("toughness", derived.toughness.to_i64())
}

/// Whether `M`'s index can name `index`.
fn named<L: Layout, M: Layout>(index: L::Index) -> Result<(), Misfit> {
    let index: usize = index.into();
    match M::Index::try_from(index) {
        Ok(_) => Ok(()),
        Err(_) => Err(Misfit::IndexTooWide { index }),
    }
}

impl GameTable {
    /// Whether layout `M`'s game index numbers every card of the table, so that a state of
    /// `M` may store any of them: asked wherever one is made.
    pub(crate) fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        if self.len() <= M::Card::COUNT {
            Ok(())
        } else {
            Err(Misfit::TooManyCards {
                cards: self.len(),
                indices: M::Card::COUNT,
            })
        }
    }
}

impl<L: Layout> State<L> {
    /// This game, whose table is `t`, in layout `M` ([`Relayout`]), if `M` holds it
    /// ([`State::fits`]).
    pub fn try_relayout<M: Layout>(&self, t: &GameTable) -> Result<State<M>, Misfit> {
        self.fits::<M>(t)?;
        Ok(self.relayout())
    }

    /// Whether layout `M` holds this game, whose table is `t`: its game index numbers every
    /// card of the table, those no object is yet among them; every object has a slot
    /// there at its place ([`SlotId::to`]); every list fits the room `M` gives it; and every
    /// recorded index is one `M` can name. A game that came from an `M` game always fits.
    ///
    /// Every field of the state is named here, so a new field does not compile until it says
    /// whether it fits, as [`Relayout`] would otherwise panic on it.
    #[allow(clippy::too_many_lines)] // A line for every field of the state.
    pub fn fits<M: Layout>(&self, t: &GameTable) -> Result<(), Misfit> {
        let State {
            // Each has a slot at its place in `M` ([`State::fits_slots`]).
            objects: _,
            battlefield,
            // One entry per battlefield index: it fits wherever the battlefield does.
            derived: _,
            listening: _,
            replacing: _,
            layer_inputs: _,
            static_rules: _,
            // A library and a graveyard hold only their owner's cards, or a shared one only
            // the shared deck's, so they fit wherever the decks do ([`State::fits_slots`]).
            zones: _,
            players,
            turn: _,
            stack,
            captured_words,
            triggers,
            delayed,
            last_known,
            departed_spells,
            // The stores say for themselves ([`State::stores_fit`]).
            effects: _,
            keyed: _,
            phased_out: _,
            memos: _,
            counters: _,
            trackers: _,
            // Its own match says ([`State::pending_fits`]).
            pending: _,
            knowledge: _,
            rng: _,
            repeats: _,
            states_due: _,
            profile: _,
            budgets: _,
            flags: _,
            outcome: _,
            elsewhere,
            grants,
            sideboard_taken: _,
            turn_plan,
            // Asked as a game of its own, below.
            outer: _,
            subgame_turns,
            restarted_turns: _,
        } = self;
        t.fits::<M>()?;
        if !M::Outer::SUPPORTED && (self.in_subgame() || subgame_turns.get() != 0) {
            return Err(Misfit::Subgame);
        }
        if let Some(outer) = self.suspended_game() {
            outer.fits::<M>(t)?;
        }
        self.fits_slots::<M>()?;
        for slot in L::Slot::all() {
            let object = self.object(slot);
            if object != &Object::EMPTY {
                fits_number::<M::Count>("marked damage", object.damage.to_i64())?;
                object.counters.fits::<M>()?;
            }
        }
        for (i, &slot) in battlefield.iter().enumerate() {
            room(
                "a blocker's attackers",
                self.object(slot).blocked_attackers().len(),
                crate::object::SlotSet::<M>::CAPACITY,
            )?;
            fits_derived::<L, M>(&self.derived[i])?;
        }
        let lists = [
            (
                "the battlefield",
                battlefield.len(),
                capacity(|s: &State<M>| &s.battlefield),
            ),
            ("the stack", stack.len(), capacity(|s: &State<M>| &s.stack)),
            (
                "the stack's captured text changes",
                captured_words.len(),
                capacity(|s: &State<M>| &s.captured_words),
            ),
            (
                "the triggered abilities waiting",
                triggers.len(),
                capacity(|s: &State<M>| &s.triggers),
            ),
            (
                "the delayed triggers",
                delayed.len(),
                capacity(|s: &State<M>| &s.delayed),
            ),
            (
                "the last-known information",
                last_known.len(),
                capacity(|s: &State<M>| &s.last_known),
            ),
            (
                "the spells kept as they left the stack",
                departed_spells.len(),
                capacity(|s: &State<M>| &s.departed_spells),
            ),
            (
                "the effects that reach other zones",
                elsewhere.len(),
                capacity(|s: &State<M>| &s.elsewhere),
            ),
            (
                "the effects that grant abilities",
                grants.len(),
                capacity(|s: &State<M>| &s.grants),
            ),
        ];
        for (list, len, capacity) in lists {
            room(list, len, capacity)?;
        }
        for entry in stack.iter() {
            room(
                "a stack entry's targets",
                entry.targets.len(),
                capacity(|e: &StackEntry<M>| &e.targets),
            )?;
            if M::Spent::of(entry.spent_on_x()).is_none() {
                return Err(Misfit::SpentOnX);
            }
        }
        for row in last_known.iter() {
            row.copiable.fits::<M>()?;
            fits_derived::<L, M>(&row.derived)?;
            row.counters.fits::<M>()?;
        }
        for row in departed_spells.iter() {
            row.copiable.fits::<M>()?;
            room(
                "a kept spell's targets",
                row.entry.targets.len(),
                capacity(|e: &StackEntry<M>| &e.targets),
            )?;
        }
        turn_plan.fits::<M>()?;
        self.stores_fit::<M>()?;
        for player in players.iter() {
            fits_number::<M::Life>("life total", player.life.to_i64())?;
            player.mana.fits::<M>()?;
            room(
                "a player's emblems",
                player.emblems.len(),
                capacity(|p: &Player<M>| &p.emblems),
            )?;
            room(
                "a player's mana that remembers its source",
                player.remembered.len(),
                capacity(|p: &Player<M>| &p.remembered),
            )?;
        }
        self.pending_fits::<M>()
    }

    fn pending_fits<M: Layout>(&self) -> Result<(), Misfit> {
        match &self.pending {
            Pending::Resolving(paused)
            | Pending::StepActions(paused)
            | Pending::TurnStart(paused) => Self::fits_paused::<M>(paused),
            Pending::CombatDamage { lethal, .. } => room(
                "combat damage assignments",
                lethal.len(),
                <M::Queue<crate::pending::LethalAssignment<M>, 32> as Queue<_>>::CAPACITY,
            ),
            Pending::ExactCombatDamage(dividing) => room(
                "combat damage shares",
                dividing.shares.len(),
                <M::Queue<crate::pending::DamageShare<M>, 32> as Queue<_>>::CAPACITY,
            ),
            Pending::None
            | Pending::PlayOrDraw { .. }
            | Pending::Mulligans
            | Pending::Discard
            | Pending::LegendRule { .. }
            | Pending::EnterPayingLife { .. }
            | Pending::DeclareAttackers { .. }
            | Pending::DeclareBlockers { .. }
            | Pending::Casting(_)
            | Pending::TurningFaceUp { .. }
            | Pending::OrderEntered { .. }
            | Pending::EnterChoosing { .. }
            | Pending::ManaColor { .. }
            | Pending::Outlet { .. }
            | Pending::ManaType { .. }
            | Pending::ManaPrice { .. }
            | Pending::Companion { .. }
            | Pending::CompanionPayment { .. }
            | Pending::PayingToAttack { .. }
            | Pending::ArrangePiles { .. } => Ok(()),
        }
    }

    /// Whether every object of this game has a slot in `M`: a library and a graveyard hold
    /// only their owner's cards, so they fit whenever the decks do.
    fn fits_slots<M: Layout>(&self) -> Result<(), Misfit> {
        let lost = L::Slot::all().find(|&slot| {
            slot.to::<M::Slot>().is_none() && self.objects[slot.index()] != Object::EMPTY
        });
        match lost.map(SlotId::place) {
            None => Ok(()),
            Some(SlotPlace::Deck(player, _)) => Err(Misfit::DeckTooLarge {
                player,
                cards: (0..L::Slot::DECK_SLOTS)
                    .filter(|&i| self.objects[L::Slot::deck(player, i).index()] != Object::EMPTY)
                    .count(),
                slots: M::Slot::DECK_SLOTS,
            }),
            Some(SlotPlace::Pool(_)) => Err(Misfit::PoolTooSmall {
                slots: M::Slot::COUNT - M::Slot::POOL,
            }),
        }
    }

    /// Whether `M` holds a paused resolution: its record and its question.
    fn fits_paused<M: Layout>(paused: &Resolving<L>) -> Result<(), Misfit> {
        room(
            "a paused resolution's answers",
            paused.record.len(),
            capacity(|r: &Resolving<M>| &r.record),
        )?;
        for (answer, hidden) in paused.recorded_answers() {
            if hidden {
                if hidden_pick_of::<L>(answer).is_some_and(|slot| slot.to::<M::Slot>().is_none()) {
                    return Err(Misfit::IndexTooWide {
                        index: answer.into(),
                    });
                }
            } else {
                named::<L, M>(answer)?;
            }
        }
        let Candidates::Options(options) = &paused.question.candidates else {
            return Ok(());
        };
        let listed = capacity(|c: &Candidates<M>| match c {
            Candidates::Options(options) => options,
            Candidates::Objects(_) => unreachable!("asked for its type alone"),
        });
        room("a decision's options", options.len(), listed)?;
        for option in options.iter() {
            if let &Opt::Trigger(i) | &Opt::Plan(i) = option {
                named::<L, M>(i)?;
            }
        }
        Ok(())
    }
}
