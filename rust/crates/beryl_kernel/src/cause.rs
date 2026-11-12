//! What an event came from (architecture §8.1, plan K23): the [`Tag`] a recording sink keeps
//! beside each event, and the [`Scopes`] the rules open around what they do so the sink can
//! write it. Search's sink keeps no event and so no tag: opening a scope costs it one branch.

use crate::ids::Seat;
use crate::layout::{Layout, Small};
use crate::object::ObjectRef;

/// A spell or ability on the stack, by its place among those the events have said arrived
/// and have not yet said left, the first to arrive being 0
/// ([CR 405.2](https://yawgatog.com/resources/magic-rules/#R4052): "The stack keeps track of
/// the order that spells and/or abilities were added to it. Each time an object is put on
/// the stack, it's put on top of all objects already there.").
///
/// A reader follows the stack event by event: [`Event::StackArrival`] puts an entry on top,
/// [`Event::StackDeparture`] takes the one it names out, and the places above it fall by one.
/// A spell or ability that is resolving keeps its place until its departure, the last event
/// of its resolution, though the state has taken it off the stack by then. So a place is
/// the state's own index into its stack except during a resolution, where the entries that
/// arrived after the resolving one sit one higher than the state has them.
///
/// The state gives its entries no identity of their own: a field there would be copied in
/// every rollout.
///
/// [`Event::StackArrival`]: crate::event::Event::StackArrival
/// [`Event::StackDeparture`]: crate::event::Event::StackDeparture
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StackIx(pub u8);

impl StackIx {
    /// The place that is `index` in a list of entries, bottom first.
    pub fn at(index: usize) -> Self {
        Self(u8::try_from(index).unwrap_or(u8::MAX))
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// What made an event happen, as the rules say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cause<L: Layout = Small> {
    /// The game itself: a turn-based or state-based action
    /// ([CR 703.1](https://yawgatog.com/resources/magic-rules/#R7031): "Turn-based actions
    /// are game actions that happen automatically when certain steps or phases begin, or
    /// when each step and phase ends."
    /// [CR 704.1](https://yawgatog.com/resources/magic-rules/#R7041): "State-based actions
    /// are game actions that happen automatically whenever certain conditions (listed below)
    /// are met."), or a step of starting the game.
    Rules,
    /// A player's own action: a special action
    /// ([CR 116.1](https://yawgatog.com/resources/magic-rules/#R1161): "Special actions are
    /// actions a player may take when they have priority that don't use the stack. These are
    /// not to be confused with turn-based actions and state-based actions, which the game
    /// generates automatically."), casting a spell, activating an ability or putting a
    /// triggered one on the stack, and paying a cost.
    ///
    /// `entry` is the spell or ability the action casts, activates, puts on the stack or
    /// pays for, if there is one: a cost's taps and sacrifices name what they pay for
    /// ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h): "The player pays the
    /// total cost."). The event that moves a spell's card to the stack names the place its
    /// entry is about to take
    /// ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "a player first
    /// moves that card (or that copy of a card) from where it is to the stack. It becomes the
    /// topmost object on the stack.").
    Player {
        player: Seat,
        entry: Option<StackIx>,
    },
    /// The effect of the spell or ability `entry`, which is resolving
    /// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c): "The controller of
    /// the spell or ability follows its instructions in the order written."), and the last
    /// parts of its resolution: a permanent spell entering the battlefield, the spell put
    /// into its owner's graveyard or the ability removed from the stack (CR 608.2n, 608.3).
    Effect { entry: StackIx },
    /// The effect of an ability of `source` that never was on the stack: a mana ability
    /// ([CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b): "An activated mana
    /// ability doesn't go on the stack, so it can't be targeted, countered, or otherwise
    /// responded to. Rather, it resolves immediately after it is activated."), or a static
    /// ability's replacement effect
    /// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c)).
    ///
    /// `entry` is the spell or ability whose payment the mana ability was activated in, if
    /// any ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may
    /// activate an activated mana ability whenever they have priority, whenever they are
    /// casting a spell or activating an ability that requires a mana payment, or whenever a
    /// rule or effect asks for a mana payment"): what its mana pays for.
    Object {
        source: ObjectRef<L>,
        entry: Option<StackIx>,
    },
}

impl<L: Layout> Cause<L> {
    /// The spell or ability on the stack the cause names, if it names one.
    pub fn entry(&self) -> Option<StackIx> {
        match *self {
            Cause::Rules => None,
            Cause::Player { entry, .. } | Cause::Object { entry, .. } => entry,
            Cause::Effect { entry } => Some(entry),
        }
    }

    fn entry_mut(&mut self) -> Option<&mut StackIx> {
        match self {
            Cause::Rules => None,
            Cause::Player { entry, .. } | Cause::Object { entry, .. } => entry.as_mut(),
            Cause::Effect { entry } => Some(entry),
        }
    }
}

/// What a recording sink keeps beside an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tag<L: Layout = Small> {
    pub cause: Cause<L>,
    /// Whether the event happened as one event of the rules with the event kept before it:
    /// the two are of one group ([`Tag::groups`]). The rules open a group around what they
    /// do at once ([`Sink::together`]), or say so of each event of a batch after its first
    /// ([`Sink::push_joined`]), as
    /// [CR 704.3](https://yawgatog.com/resources/magic-rules/#R7043) has the game perform
    /// "all applicable state-based actions simultaneously as a single event", and every
    /// event kept until it closes is of it; an event kept with no group open is a group of
    /// its own.
    ///
    /// A group has no number of its own, only this word about its neighbor: a body that runs
    /// again from its start (architecture §7.3) keeps the same events with the same tags as
    /// its run before, which a count of the groups so far would not be.
    ///
    /// [`Sink::together`]: crate::event::Sink::together
    /// [`Sink::push_joined`]: crate::event::Sink::push_joined
    pub together: bool,
}

impl<L: Layout> Tag<L> {
    /// The group of each of `tags`, in step with them, numbered from 0 in the order the
    /// groups began: two events have the same number exactly when they happened as one
    /// event of the rules. The first of a list cut from a longer one begins a group here,
    /// whatever it was together with before the cut.
    pub fn groups<'a>(tags: impl IntoIterator<Item = &'a Tag<L>>) -> Vec<u32> {
        let mut next = 0;
        let groups = tags.into_iter().enumerate().map(|(i, tag)| {
            if i > 0 && !tag.together {
                next += 1;
            }
            next
        });
        groups.collect()
    }
}

/// The causes of what the rules are doing now, the innermost last, and none while the game
/// acts by itself. They nest: a mana ability activated to pay a cost an effect asks for sits
/// inside that payment, inside the resolution (CR 608.2g).
#[derive(Debug)]
pub(crate) struct Scopes<L: Layout> {
    open: Vec<Cause<L>>,
    /// The place of the spell or ability that is resolving, which the state has taken off
    /// its stack ([`StackIx`]).
    resolving: Option<StackIx>,
    /// How many groups are open, one inside another: the outermost is the group
    /// ([`Tag::together`]).
    groups: u32,
    /// Whether an event has been kept in the open group.
    begun: bool,
}

impl<L: Layout> Default for Scopes<L> {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            resolving: None,
            groups: 0,
            begun: false,
        }
    }
}

impl<L: Layout> Scopes<L> {
    /// The tag of the event being kept now.
    pub(crate) fn tag(&mut self) -> Tag<L> {
        let together = self.groups > 0 && self.begun;
        self.begun = true;
        Tag {
            cause: self.open.last().copied().unwrap_or(Cause::Rules),
            together,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.open.is_empty() && self.resolving.is_none() && self.groups == 0
    }

    /// Opens a group. One opened inside another is part of it: what the rules do at once
    /// may be made of things they also do at once.
    ///
    /// `joined`: the group is the one of the event kept last, which what follows it is part
    /// of.
    pub(crate) fn together(&mut self, joined: bool) {
        if self.groups == 0 {
            self.begun = joined;
        }
        self.groups += 1;
    }

    pub(crate) fn apart(&mut self) {
        debug_assert!(self.groups > 0, "no group to close");
        self.groups = self.groups.saturating_sub(1);
    }

    pub(crate) fn enter(&mut self, cause: Cause<L>) {
        self.open.push(cause);
    }

    /// Opens the resolution of the top of the stack, which the state has just taken off,
    /// leaving `below` entries.
    pub(crate) fn resolve(&mut self, below: usize) {
        debug_assert!(self.resolving.is_none(), "a resolution inside another");
        let entry = StackIx::at(below);
        self.resolving = Some(entry);
        self.open.push(Cause::Effect { entry });
    }

    pub(crate) fn leave(&mut self) {
        if let Some(Cause::Effect { .. }) = self.open.pop() {
            self.resolving = None;
        }
    }

    /// The spell or ability the innermost cause names.
    pub(crate) fn entry(&self) -> Option<StackIx> {
        self.open.last().and_then(Cause::entry)
    }

    pub(crate) fn resolving(&self) -> Option<StackIx> {
        self.resolving
    }

    /// The place of the entry at `index` of the state's stack.
    pub(crate) fn place(&self, index: usize) -> StackIx {
        let above = self.resolving.is_some_and(|r| r.index() <= index);
        StackIx::at(index + usize::from(above))
    }

    /// The entry at `left` has left the stack: the places above it fall by one.
    pub(crate) fn departed(&mut self, left: StackIx) {
        let places = self.open.iter_mut().filter_map(Cause::entry_mut);
        for place in places.chain(self.resolving.as_mut()) {
            if *place > left {
                place.0 -= 1;
            }
        }
    }
}
