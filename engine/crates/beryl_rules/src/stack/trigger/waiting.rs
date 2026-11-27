//! The triggered abilities waiting to be put on the stack: which of them a player chooses
//! among next, putting one on the stack, and whether an ability is waiting or already there.
//!
//! Implements: none. Orders waiting triggers under the rules declared by its parent module.

use std::cell::OnceCell;

use super::ability;
use crate::bitboard::Bitboard;
use crate::card::TriggerEvent;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, index};
use crate::replacement::event::Sink;
use crate::state::{
    AbilityRef, ObjectRef, PendingTrigger, StackEntry, StackItem, State, TriggerInfo,
    TriggeredAbility,
};
use crate::table::GameTable;

/// Whether a triggered ability that `is` accepts is waiting to be put on the stack or is on it.
pub(crate) fn waiting_or_on_stack<L: Layout>(
    s: &State<L>,
    is: impl Fn(&AbilityRef<L, TriggeredAbility>) -> bool,
) -> bool {
    s.triggers().iter().any(|tr| is(&tr.of))
        || (s.stack().iter())
            .any(|entry| matches!(&entry.item, StackItem::Triggered { of, .. } if is(of)))
}

/// The player who puts the next waiting trigger on the stack, and the triggers they choose
/// from: one of each set of equivalent ones (§7.2), as indices into the waiting triggers.
///
/// [CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b): "each player, in APNAP order,
/// puts each triggered ability they control ... on the stack in any order they choose."
pub(crate) fn next_to_stack<L: Layout>(s: &State<L>) -> Option<(Seat, L::Queue<L::Index, 64>)> {
    let player = s
        .players_in_turn_order(s.players())
        .find(|&p| s.triggers().iter().any(|tr| tr.of.controller.seat() == p))?;
    let mut kept: L::Queue<L::Index, 64> = Default::default();
    let mut firsts: L::Queue<PendingTrigger<L>, 64> = Default::default();
    // Found only once two triggers of one ability have different sources or objects.
    let named = OnceCell::new();
    for (i, tr) in s.triggers().iter().enumerate() {
        if tr.of.controller.seat() != player || firsts.iter().any(|k| equivalent(s, &named, k, tr))
        {
            continue;
        }
        // The waiting triggers never exceed the capacity of these lists, and the layout's
        // index names every one.
        firsts.push_bounded(*tr);
        kept.push_bounded(index::<L>(i));
    }
    Some((player, kept))
}

/// Triggers of the same ability from interchangeable sources, on the same event with
/// interchangeable objects, lead to the same game whatever their order (§7.2).
///
/// Two objects are interchangeable here as two candidates are (`engine::collapse`): alike
/// in themselves ([`State::signature`]) and named by no other record of the state
/// ([`State::named`]). A waiting trigger names its own source, so the waiting triggers are
/// held to that rule another way: with one trigger's source and object renamed to the
/// other's and back, they must be the same triggers. Two abilities that each watch one of
/// two twins leave the twins alike; one ability waiting to act on one twin alone tells
/// them apart, as it will once it is on the stack.
fn equivalent<L: Layout>(
    s: &State<L>,
    named: &OnceCell<Bitboard<L::Slot>>,
    a: &PendingTrigger<L>,
    b: &PendingTrigger<L>,
) -> bool {
    if (a.of.card, a.of.face, a.of.ability, a.of.controller)
        != (b.of.card, b.of.face, b.of.ability, b.of.controller)
        || (a.info.player, a.info.amount) != (b.info.player, b.info.amount)
        || !same_last_known(s, a.info.object, b.info.object)
    {
        return false;
    }
    let pairs = [(a.of.source, b.of.source), (a.info.object, b.info.object)];
    if pairs.iter().all(|(x, y)| x == y) {
        return true;
    }
    let named = *named.get_or_init(|| s.named_apart_from_waiting_triggers());
    let alike = |x: L::Slot, y: L::Slot| {
        !named.contains(x) && !named.contains(y) && s.signature(x) == s.signature(y)
    };
    let interchangeable = |&(x, y): &(Option<ObjectRef<L>>, Option<ObjectRef<L>>)| match (x, y) {
        (Some(x), Some(y)) => x == y || alike(x.slot, y.slot),
        _ => x == y,
    };
    if !pairs.iter().all(interchangeable) {
        return false;
    }
    // The renaming that makes each trigger the other: an object of a pair for its partner.
    let renamed = |r: Option<ObjectRef<L>>| {
        let partner = |&(x, y): &(Option<ObjectRef<L>>, Option<ObjectRef<L>>)| match r {
            _ if x == y => None,
            r if r == x => Some(y),
            r if r == y => Some(x),
            _ => None,
        };
        pairs.iter().find_map(partner).unwrap_or(r)
    };
    let rename = |tr: &PendingTrigger<L>| PendingTrigger {
        of: AbilityRef {
            source: renamed(tr.of.source),
            ..tr.of
        },
        info: TriggerInfo {
            object: renamed(tr.info.object),
            ..tr.info
        },
    };
    // It is one renaming only if it makes each of the other: a creature that saw itself
    // enter is no twin of one that saw another.
    if rename(a) != *b || rename(b) != *a {
        return false;
    }
    let waiting = s.triggers();
    let count = |tr: &PendingTrigger<L>| waiting.iter().filter(|w| *w == tr).count();
    waiting.iter().all(|tr| {
        let other = rename(tr);
        other == *tr || count(tr) == count(&other)
    })
}

/// Whether `beneath`, the entry under the top of the stack, is the same trigger again as `top`:
/// both triggered abilities of the same card, face and ability with the same controller, the
/// same X and the same targets, from the same source or from two sources alike in themselves
/// ([`State::signature`]). It decides only whether a player who passed on `top` is asked
/// again before `beneath` resolves (`Profile::passes_identical_triggers`), and a pass is always
/// legal, so it need not be the exact equivalence of waiting triggers ([`next_to_stack`]).
///
/// See [CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d): "If a player has
/// priority and chooses not to take any actions, that player passes."
#[cold]
#[inline(never)]
pub(crate) fn same_trigger_again<L: Layout>(
    s: &State<L>,
    top: &StackEntry<L>,
    beneath: &StackEntry<L>,
) -> bool {
    let (StackItem::Triggered { of: a, .. }, StackItem::Triggered { of: b, .. }) =
        (&top.item, &beneath.item)
    else {
        return false;
    };
    let alike = |x: ObjectRef<L>, y: ObjectRef<L>| {
        x == y || (s.is_current(x) && s.is_current(y) && s.signature(x.slot) == s.signature(y.slot))
    };
    (a.card, a.face, a.ability, a.controller) == (b.card, b.face, b.ability, b.controller)
        && (top.x, top.words) == (beneath.x, beneath.words)
        && top.targets.as_slice() == beneath.targets.as_slice()
        && match (a.source, b.source) {
            (Some(x), Some(y)) => alike(x, y),
            (x, y) => x == y,
        }
}

/// Whether two objects that triggers name were alike as they last existed on the
/// battlefield, where either has left it and has last-known information.
fn same_last_known<L: Layout>(
    s: &State<L>,
    a: Option<ObjectRef<L>>,
    b: Option<ObjectRef<L>>,
) -> bool {
    let known = |r: Option<ObjectRef<L>>| r.and_then(|r| s.last_known(r));
    match (known(a), known(b)) {
        (None, None) => true,
        (Some(x), Some(y)) => x.alike(y),
        _ => false,
    }
}

/// Puts the waiting trigger at `index` on the stack ([CR 603.3]): "The ability becomes the topmost
/// object on the stack." Then its targets are chosen, through the casting machine (CR 603.3d).
///
/// [CR 603.3]: https://yawgatog.com/resources/magic-rules/#R6033
pub(crate) fn put_on_stack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    index: L::Index,
) -> Result<(), Full> {
    let tr = s.take_trigger(index.into());
    // An enters ability's X is its spell's (see CR 107.3m), captured as it triggered.
    let entering = (ability(t, &tr.of).trigger()).is_some_and(|trigger| {
        trigger.is_this(TriggerEvent::Enters) || trigger.is_this(TriggerEvent::EntersUntapped)
    });
    s.put_on_stack(
        sink,
        StackEntry {
            x: if entering { tr.info.amount } else { 0 },
            ..StackEntry::new(StackItem::Triggered {
                of: tr.of,
                info: tr.info,
            })
        },
    )?;
    crate::casting::target_trigger(s, t, sink)
}
