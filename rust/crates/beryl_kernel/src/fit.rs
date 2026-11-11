//! Whether a game fits a [`Layout`]: its decks the slots a player's deck is given, its tokens
//! the pool, each of its lists the room the layout gives that list. A game that does not is
//! refused where a state of the layout is made, with a [`Misfit`] and never a panic
//! ([`State::deal`], [`State::try_relayout`]).

use std::fmt;

use crate::decision::{Candidates, Opt};
use crate::ids::{CardId, Place, Seat, SlotId};
use crate::layout::{Item, Layout, Queue, Table};
use crate::object::Object;
use crate::pending::{Pending, Resolving};
use crate::player::Player;
use crate::relayout::Relayout;
use crate::stack::StackEntry;
use crate::state::State;
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
    /// A recorded answer or an option names a place in a list past what the layout's
    /// [`Layout::Index`] can name.
    IndexTooWide { index: usize },
}

impl fmt::Display for Misfit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
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
        }
    }
}

impl std::error::Error for Misfit {}

/// The room the queue `field` reads from a `V` has: its type's, so no `V` is made to ask.
fn capacity<V, T: Item, Q: Queue<T>>(_field: impl Fn(&V) -> &Q) -> usize {
    Q::CAPACITY
}

/// Whether a list of `len` fits `capacity`.
fn room(list: &'static str, len: usize, capacity: usize) -> Result<(), Misfit> {
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
    pub fn fits<M: Layout>(&self, t: &GameTable) -> Result<(), Misfit> {
        t.fits::<M>()?;
        self.fits_slots::<M>()?;
        room(
            "the battlefield",
            self.battlefield.len(),
            capacity(|s: &State<M>| &s.battlefield),
        )?;
        room(
            "the stack",
            self.stack.len(),
            capacity(|s: &State<M>| &s.stack),
        )?;
        for entry in self.stack.iter() {
            room(
                "a stack entry's targets",
                entry.targets.len(),
                capacity(|e: &StackEntry<M>| &e.targets),
            )?;
        }
        room(
            "the triggered abilities waiting",
            self.triggers.len(),
            capacity(|s: &State<M>| &s.triggers),
        )?;
        room(
            "the delayed triggers",
            self.delayed.len(),
            capacity(|s: &State<M>| &s.delayed),
        )?;
        room(
            "the continuous effects",
            self.effects.len(),
            capacity(|s: &State<M>| &s.effects),
        )?;
        room(
            "the cast permissions",
            self.permissions.len(),
            capacity(|s: &State<M>| &s.permissions),
        )?;
        room(
            "the values of X",
            self.entered_x.len(),
            capacity(|s: &State<M>| &s.entered_x),
        )?;
        for player in self.players.iter() {
            room(
                "a player's emblems",
                player.emblems.len(),
                capacity(|p: &Player<M>| &p.emblems),
            )?;
        }
        match &self.pending {
            Pending::Resolving(paused) => Self::fits_paused::<M>(paused),
            _ => Ok(()),
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
            Some(Place::Deck(player, _)) => Err(Misfit::DeckTooLarge {
                player,
                cards: (0..L::Slot::DECK_SLOTS)
                    .filter(|&i| self.objects[L::Slot::deck(player, i).index()] != Object::EMPTY)
                    .count(),
                slots: M::Slot::DECK_SLOTS,
            }),
            Some(Place::Pool(_)) => Err(Misfit::PoolTooSmall {
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
        for &answer in paused.record.iter() {
            named::<L, M>(answer)?;
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
