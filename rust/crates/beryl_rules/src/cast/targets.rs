//! Choosing targets (CR 601.2c) and whether they are still legal (CR 115.3, CR 608.2b).

use crate::PlayerSet;
use crate::bitboard::Bitboard;
use crate::card::{Ability, TargetKind, TargetSpec};
use crate::event::Sink;
use crate::filter::{self, Evaluate, Filter};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::state::{CastKind, CastStep, Casting, StackEntry, State, Target, Zone};
use crate::table::GameTable;

use super::steps::{ability_event, advance, specs, top};
use super::timing::entry_ability;

/// The objects or players a target spec can choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetSet<L: Layout = Small> {
    Objects(Bitboard<L::Slot>),
    Players(PlayerSet),
}

fn player_matches(filter: Filter, player: Seat, you: Seat) -> bool {
    match filter {
        Filter::Any => true,
        Filter::Whose(players) => filter::is_for(players, player, you),
        _ => unreachable!("a player filter is Any or Whose"),
    }
}

/// The spells on the stack, as objects.
pub(crate) fn spells<L: Layout>(s: &State<L>) -> Bitboard<L::Slot> {
    s.stack()
        .iter()
        .filter_map(StackEntry::spell)
        .map(|r| r.slot)
        .collect()
}

/// The objects or players a target spec can choose now, for a spell or ability controlled by
/// `you`. `itself` is the spell being cast, if one is, which is an illegal target for itself
/// ([CR 115.5](https://yawgatog.com/resources/magic-rules/#R1155)): "A spell or ability on the
/// stack is an illegal target for itself." Before it is cast it is not yet on the stack, but
/// it will have left the zone it is in when its targets are chosen. `x` is the value of X of
/// the spell or ability. `source` is an ability's source, which "another target" can't
/// choose.
pub(super) fn candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<L::Slot>,
    x: u8,
    source: Option<L::Slot>,
) -> TargetSet<L> {
    match domain(s, spec, you, itself, source) {
        TargetSet::Objects(within) => {
            TargetSet::Objects(spec.filter.select_x(s, t, within, you, x))
        }
        players @ TargetSet::Players(_) => players,
    }
}

/// What [`candidates`] chooses from: the players a spec can choose, or the objects its filter
/// is still to pass on.
fn domain<L: Layout>(
    s: &State<L>,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<L::Slot>,
    source: Option<L::Slot>,
) -> TargetSet<L> {
    let mut within = match spec.kind {
        TargetKind::Player => {
            let players = s.players().filter(|p| player_matches(spec.filter, p, you));
            return TargetSet::Players(players);
        }
        TargetKind::Permanent => s.in_zone(Zone::Battlefield),
        TargetKind::Spell => spells(s),
        TargetKind::SpellOrPermanent => spells(s) | s.in_zone(Zone::Battlefield),
        TargetKind::GraveyardCard => s.in_zone(Zone::Graveyard),
    };
    if let Some(itself) = itself {
        within.remove(itself);
    }
    if let Some(source) = source.filter(|_| spec.other) {
        within.remove(source);
    }
    TargetSet::Objects(within)
}

/// Whether [`candidates`] holds at least `spec.count` targets, found without testing the
/// filter on more objects than that takes.
fn enough_candidates<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    you: Seat,
    itself: Option<L::Slot>,
    x: u8,
    source: Option<L::Slot>,
) -> bool {
    let need = usize::from(spec.count);
    match domain(s, spec, you, itself, source) {
        TargetSet::Objects(within) => {
            within.at_least(need, |slot| spec.filter.matches_x(s, t, slot, you, x))
        }
        TargetSet::Players(players) => players.count() as usize >= need,
    }
}

impl<L: Layout> TargetSet<L> {
    pub(super) fn count(self) -> u32 {
        match self {
            TargetSet::Objects(b) => b.count(),
            TargetSet::Players(p) => p.count(),
        }
    }
}

/// Whether every required target can be chosen
/// ([CR 115.3](https://yawgatog.com/resources/magic-rules/#R1153)): "The same target can't be
/// chosen multiple times for any one instance of the word "target" on a spell or ability."
pub(super) fn targetable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    ability: Option<&Ability>,
    you: Seat,
    itself: Option<L::Slot>,
    x: u8,
    source: Option<L::Slot>,
) -> bool {
    ability.is_none_or(|a| {
        a.targets()
            .iter()
            .all(|spec| spec.optional || enough_candidates(s, t, spec, you, itself, x, source))
    })
}

/// The slot of an entry's ability source; a spell has none.
pub(super) fn source_of<L: Layout>(entry: &StackEntry<L>) -> Option<L::Slot> {
    entry.source().map(|r| r.slot)
}

/// Whether a chosen target is still legal as its spell or ability resolves
/// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "A target that's no
/// longer in the zone it was in when it was targeted is illegal. Other changes to the game
/// state may cause a target to no longer be legal". A zone change gives the object a new
/// incarnation, so a stale reference is illegal.
pub(crate) fn still_legal<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    entry: &StackEntry<L>,
    ability: &Ability,
    target: Target<L>,
) -> bool {
    let spec = &ability.targets()[target.spec().index()];
    let you = entry.controller();
    let Some(r) = target.as_object() else {
        return target
            .as_player()
            .is_some_and(|p| player_matches(spec.filter, p, you));
    };
    if !s.is_current(r) {
        return false;
    }
    let itself = entry.spell().map(|r| r.slot);
    match candidates(s, t, spec, you, itself, entry.x, source_of(entry)) {
        TargetSet::Objects(b) => b.contains(r.slot),
        TargetSet::Players(_) => false,
    }
}

/// Chooses the targets of the triggered ability just put on the stack, if it has any.
///
/// [CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "The remainder of the
/// process for putting a triggered ability on the stack is identical to the process for
/// casting a spell listed in rules 601.2c-d. If a choice is required when the triggered
/// ability goes on the stack but no legal choices can be made for it, ... the ability is
/// simply removed from the stack."
pub(crate) fn target_trigger<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let entry = top(s);
    let Some(ability) = entry_ability(t, entry) else {
        return Ok(());
    };
    if ability.targets().is_empty() {
        // The trigger is already on the stack, with no choices to announce first
        // ([CR 603.3](https://yawgatog.com/resources/magic-rules/#R6033)):
        // "its controller puts it on the stack as an object that's not a card".
        sink.note(ability_event(entry));
        return Ok(());
    }
    if !targetable(
        s,
        t,
        Some(ability),
        entry.controller(),
        None,
        entry.x,
        source_of(entry),
    ) {
        s.take_top_of_stack();
        sink.departed(|| s.stack().len());
        return Ok(());
    }
    let c = Casting::new(entry.controller(), CastKind::Trigger, CastStep::TargetCount);
    advance(s, t, sink, c)
}

/// The objects or players the current target spec can still choose: not one already chosen
/// for it.
pub(super) fn target_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    c: &Casting<L>,
) -> TargetSet<L> {
    let entry = top(s);
    let spec = &specs(t, entry)[c.spec.index()];
    let itself = entry.spell().map(|r| r.slot);
    match candidates(s, t, spec, c.player, itself, entry.x, source_of(entry)) {
        TargetSet::Objects(mut b) => {
            for target in entry.targets.iter().filter(|x| x.spec() == c.spec) {
                if let Some(r) = target.as_object() {
                    b.remove(r.slot);
                }
            }
            TargetSet::Objects(b)
        }
        TargetSet::Players(mut p) => {
            for target in entry.targets.iter().filter(|x| x.spec() == c.spec) {
                if let Some(player) = target.as_player() {
                    p.remove(player);
                }
            }
            TargetSet::Players(p)
        }
    }
}
