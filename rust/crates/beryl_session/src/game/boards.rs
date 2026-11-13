//! The game's board in the layout it is in, and the copies of a state that undo points hold.

use std::sync::Arc;

use beryl_engine::chance::Chance;
use beryl_engine::event::Tagged;
use beryl_engine::state::{Pending, StackEntry, Turn};
use beryl_engine::{
    Choice, GameTable, Large, Layout, Outcome, Profile, Relayout, Replay, Scratch, Small,
    StallReason, State, Step, TurnStep, apply, next_in_step,
};

/// The game's state and the engine's working memory for it, in one layout.
pub(super) struct Board<L: Layout> {
    pub(super) state: State<L>,
    pub(super) scratch: Scratch<L>,
    /// Working memory for looking ahead in a copy of the state, whose events are dropped.
    pub(super) peek: Scratch<L>,
}

/// The game in the layout it is in (architecture §4.1): [`Small`], the layout search runs on,
/// until a capacity of it runs out, then [`Large`] for good. The game is widened as it stood
/// before the engine call that ran out, and that call is made again in `Large`, so no player
/// game ends stalled for a capacity of `Small` and no player sees the difference.
pub(super) enum Boards {
    Small(Box<Board<Small>>),
    Large(Box<Board<Large>>),
}

/// Evaluates `$body` with `$b` bound to the board, whichever layout it is in.
macro_rules! on_board {
    ($boards:expr, $b:ident => $body:expr) => {
        match $boards {
            Boards::Small($b) => $body,
            Boards::Large($b) => $body,
        }
    };
}

pub(super) use on_board;

/// Runs `f` with `chance` lent to the scratch's sink ([`beryl_engine::chance`]), and takes it
/// back after.
pub(super) fn lend<L: Layout, R>(
    x: &mut Scratch<L>,
    chance: &mut Chance,
    f: impl FnOnce(&mut Scratch<L>) -> R,
) -> R {
    x.events.set_chance(Some(std::mem::take(chance)));
    let r = f(x);
    *chance = x.events.take_chance().expect("the chance lent");
    r
}

/// Whether the engine stalled the game because a capacity of its layout ran out.
pub(super) fn ran_out<L: Layout>(s: &State<L>) -> bool {
    s.outcome() == Some(Outcome::Stalled(StallReason::Capacity))
}

/// A copy of the game's state, in the layout it was in, shared: an undo point and the cast
/// begun from it ([`BeforeCast`]) hold one copy between them.
#[derive(Clone)]
pub(super) enum Snapshot {
    Small(Arc<State<Small>>),
    Large(Arc<State<Large>>),
}

impl Boards {
    pub(super) fn new(state: &State<Small>) -> Boards {
        Boards::Small(Box::new(Board {
            state: *state,
            scratch: Scratch::recording(),
            peek: Scratch::new(),
        }))
    }

    /// Goes on in [`Large`] from `before`, the state before the engine call that ran out of
    /// capacity, keeping the events the scratch had recorded by then: the first `events`.
    pub(super) fn widen(&mut self, before: &State<Small>, events: usize) {
        let Boards::Small(b) = self else {
            unreachable!("only a Small game is widened");
        };
        let mut scratch = Scratch::new();
        scratch.events = b.scratch.events.relayout();
        scratch.events.truncate(events);
        *self = Boards::Large(Box::new(Board {
            state: before.widen(),
            scratch,
            peek: Scratch::new(),
        }));
    }

    /// Applies `c` to the open decision with the game's `chance`, widening the game if it
    /// runs out of capacity.
    pub(super) fn apply(&mut self, t: &GameTable, c: Choice, chance: &mut Chance) {
        if let Boards::Small(b) = self {
            let (before, events, lent) = (b.state, b.scratch.events.len(), chance.clone());
            lend(&mut b.scratch, chance, |x| {
                apply(&mut b.state, t, x, &mut Replay, c);
            });
            if !ran_out(&b.state) {
                return;
            }
            *chance = lent;
            self.widen(&before, events);
        }
        let Boards::Large(b) = self else {
            unreachable!("a game widened")
        };
        lend(&mut b.scratch, chance, |x| {
            apply(&mut b.state, t, x, &mut Replay, c);
        });
    }

    /// Plays on as [`next_in_step`] does with the game's `chance`, widening the game if it
    /// runs out of capacity.
    pub(super) fn next_in_step(
        &mut self,
        t: &GameTable,
        seen: (u16, TurnStep),
        chance: &mut Chance,
    ) -> Option<Step<Large>> {
        if let Boards::Small(b) = self {
            let (before, events, lent) = (b.state, b.scratch.events.len(), chance.clone());
            let step = lend(&mut b.scratch, chance, |x| {
                next_in_step(&mut b.state, t, x, &mut Replay, seen)
            });
            if !ran_out(&b.state) {
                return step.map(|s| s.relayout());
            }
            *chance = lent;
            self.widen(&before, events);
        }
        let Boards::Large(b) = self else {
            unreachable!("a game widened")
        };
        lend(&mut b.scratch, chance, |x| {
            next_in_step(&mut b.state, t, x, &mut Replay, seen)
        })
    }

    /// What [`Boards::next_in_step`] would return, played on a copy with a copy of `chance`,
    /// so a replay looks ahead at the cards its record has.
    pub(super) fn ahead(
        &mut self,
        t: &GameTable,
        seen: (u16, TurnStep),
        chance: &Chance,
    ) -> Option<Step<Large>> {
        match self {
            Boards::Small(b) => {
                let mut ahead = b.state;
                let step = lend(&mut b.peek, &mut chance.clone(), |x| {
                    next_in_step(&mut ahead, t, x, &mut Replay, seen)
                });
                if !ran_out(&ahead) {
                    return step.map(|s| s.relayout());
                }
                let mut wide = b.state.widen();
                lend(&mut Scratch::new(), &mut chance.clone(), |x| {
                    next_in_step(&mut wide, t, x, &mut Replay, seen)
                })
            }
            Boards::Large(b) => {
                let mut ahead = b.state.clone();
                lend(&mut b.peek, &mut chance.clone(), |x| {
                    next_in_step(&mut ahead, t, x, &mut Replay, seen)
                })
            }
        }
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        match self {
            Boards::Small(b) => Snapshot::Small(Arc::new(b.state)),
            Boards::Large(b) => Snapshot::Large(Arc::new(b.state.clone())),
        }
    }

    /// Puts the game back as `snapshot` holds it, in the layout the game is in now: a game
    /// widened since keeps its layout.
    pub(super) fn restore(&mut self, snapshot: &Snapshot) {
        match (self, snapshot) {
            (Boards::Small(b), Snapshot::Small(s)) => b.state = **s,
            (Boards::Large(b), Snapshot::Small(s)) => b.state = s.widen(),
            (Boards::Large(b), Snapshot::Large(s)) => b.state = State::clone(s),
            (Boards::Small(_), Snapshot::Large(_)) => unreachable!("a game is never narrowed"),
        }
    }

    /// The events the engine recorded since the last drain, each with its tag.
    pub(super) fn drain_events(&mut self) -> Vec<Tagged<Large>> {
        on_board!(self, b => b.scratch.events.drain_tagged().map(|e| e.relayout()).collect())
    }

    /// Whether a spell is being cast or an ability activated or put on the stack, and if so
    /// whether it is a triggered ability, putting its targets.
    pub(super) fn casting(&self) -> Option<bool> {
        on_board!(self, b => match b.state.pending() {
            Pending::Casting(c) => Some(c.triggered()),
            _ => None,
        })
    }

    /// Whether a resolution is paused for an answer (§7.3).
    pub(super) fn resolving(&self) -> bool {
        on_board!(self, b => matches!(b.state.pending(), Pending::Resolving(_)))
    }

    pub(super) fn stack(&self) -> Vec<StackEntry<Large>> {
        on_board!(self, b => b.state.stack().iter().map(Relayout::relayout).collect())
    }

    pub(super) fn stack_is_empty(&self) -> bool {
        on_board!(self, b => b.state.stack().is_empty())
    }

    pub(super) fn turn(&self) -> &Turn {
        on_board!(self, b => b.state.turn())
    }

    /// Whether a creature is blocking.
    pub(super) fn any_blocking(&self) -> bool {
        on_board!(self, b => b
            .state
            .battlefield()
            .iter()
            .any(|&slot| b.state.object(slot).is_blocking()))
    }

    pub(super) fn outcome(&self) -> Option<Outcome> {
        on_board!(self, b => b.state.outcome())
    }

    pub(super) fn profile(&self) -> Profile {
        on_board!(self, b => b.state.profile())
    }

    pub(super) fn set_profile(&mut self, profile: Profile) {
        on_board!(self, b => b.state.set_profile(profile));
    }
}
