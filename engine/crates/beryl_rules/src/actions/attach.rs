//! Attach ([CR 701.3](https://yawgatog.com/resources/magic-rules/#R7013)).
//!
//! Implements: CR 301.5c, CR 303.4d, CR 701.3, CR 702.5a, CR 702.5c, CR 702.16c, CR 702.16d

use beryl_card::{AbilityKind, Objects, TargetKind, TargetSpec};

use crate::card::{Subtype, Types};
use crate::filter::Evaluate;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::replacement::protection;
use crate::state::State;
use crate::table::GameTable;

/// Whether the Aura in `aura`, on the battlefield, may enchant the object in `host`, on the
/// battlefield or in a graveyard.
///
/// [CR 702.5c](https://yawgatog.com/resources/magic-rules/#R7025c): "If an Aura has
/// multiple instances of enchant, all of them apply. ... The Aura can enchant only objects or
/// players that match all of its enchant abilities." An
/// Aura's enchant abilities are what the spell abilities it has now target
/// ([`State::abilities`](crate::state::State::abilities)): its own, less one an effect
/// removed, and one an effect granted, as Animate Dead loses "enchant creature card in a
/// graveyard" and gains "enchant creature put onto the battlefield with this Aura". An Aura
/// with none left is restricted by none.
///
/// [CR 303.4d](https://yawgatog.com/resources/magic-rules/#R3034d): "An Aura can't enchant
/// itself. ... An Aura that's also a creature can't enchant anything."
pub(crate) fn may_enchant<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    aura: L::Slot,
    host: L::Slot,
) -> bool {
    if aura == host || is_creature(s, aura) {
        return false;
    }
    may_enchant_host(s, t, aura, |spec| enchantable(s, t, spec, aura, host))
}

/// Whether the Aura in `aura` is also a creature, which can't enchant anything (CR 303.4d).
fn is_creature<L: Layout>(s: &State<L>, aura: L::Slot) -> bool {
    s.derived(aura).types.contains(Types::CREATURE)
}

/// Whether an Aura can enchant a player. Enchant abilities govern the attachment
/// ([CR 702.5a]): "The enchant ability restricts what an Aura spell can target
/// and what an Aura can enchant." Protection also restricts the attachment, even
/// if the Aura entered without targeting (see [CR 702.16j]): "Such a permanent or
/// player can't be targeted by spells or abilities and can't be enchanted by Auras."
///
/// [CR 702.5a]: https://yawgatog.com/resources/magic-rules/#R7025a
/// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
pub(crate) fn may_enchant_player<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    aura: L::Slot,
    player: Seat,
) -> bool {
    s.players().contains(player)
        && !is_creature(s, aura)
        && !crate::continuous::rule_change::protected_from_everything(s, t, player)
        && may_enchant_host(s, t, aura, |spec| {
            !spec.put_with_this && crate::casting::player_matches(spec, player, s.controller(aura))
        })
}

fn may_enchant_host<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    aura: L::Slot,
    allows: impl Fn(&TargetSpec) -> bool,
) -> bool {
    if s.derived(aura).types.contains(Types::CREATURE) {
        return false;
    }
    s.abilities(t, aura)
        .filter(|(_, ability)| ability.kind() == AbilityKind::Spell)
        .flat_map(|(_, ability)| {
            crate::coverage::hit(ability);
            ability.targets()
        })
        .all(allows)
}

/// Whether one enchant ability, the target spec of an Aura's spell ability, allows the Aura in
/// `aura` to enchant the object in `host`: an object in the zone the spec chooses from, which
/// passes its filter read for the Aura's controller and is not protected from the Aura
/// ([CR 702.16c]): "A permanent or player with protection can’t be enchanted by Auras that
/// have the stated quality."
///
/// [CR 702.16c]: https://yawgatog.com/resources/magic-rules/#R70216c
fn enchantable<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    spec: &TargetSpec,
    aura: L::Slot,
    host: L::Slot,
) -> bool {
    let zone = s.object(host).zone();
    let in_zone = match spec.kind {
        TargetKind::Permanent | TargetKind::PermanentOrPlayer | TargetKind::SpellOrPermanent => {
            zone == crate::state::Zone::Battlefield
        }
        TargetKind::GraveyardCard => zone == crate::state::Zone::Graveyard,
        // This host is an object. The engine has no attachment to a spell or ability.
        TargetKind::Player
        | TargetKind::Spell
        | TargetKind::Ability
        | TargetKind::ActivatedAbility
        | TargetKind::TriggeredAbility => false,
    };
    in_zone
        && (!spec.put_with_this
            || s.put_onto_battlefield_with(s.object_ref(aura))
                .contains(host))
        && spec
            .filter
            .matches_x(s, t, host, s.controller(aura), &s.words_of(aura), 0)
        && !protection::protected(s, t, host, aura)
}

/// Whether the Equipment in `equipment` may be attached to the permanent in `host`, both on
/// the battlefield.
///
/// [CR 301.5c](https://yawgatog.com/resources/magic-rules/#R3015c): "An Equipment that's also
/// a creature can't equip a creature unless that Equipment has reconfigure ... An Equipment
/// that loses the subtype 'Equipment' can't equip a creature. An Equipment can't equip
/// itself." No implemented card has reconfigure.
///
/// [CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b): "If an effect tries to
/// attach an object that isn't an Aura, Equipment, or Fortification to another object or
/// player, the effect does nothing and the first object doesn't move." An Aura is asked
/// whether its enchant abilities allow the host instead.
///
/// [CR 702.16d](https://yawgatog.com/resources/magic-rules/#R70216d): "A permanent with
/// protection can’t be equipped by Equipment that have the stated quality".
pub(crate) fn may_equip_in<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    equipment: L::Slot,
    host: L::Slot,
) -> bool {
    let (gear, creature) = (s.derived(equipment), s.derived(host));
    let is_equipment = t.subtypes().has(Subtype::Equipment).of(gear.subtypes);
    is_equipment
        && equipment != host
        && !gear.types.contains(Types::CREATURE)
        && creature.types.contains(Types::CREATURE)
        && !protection::protected(s, t, host, equipment)
}

/// [`Effect::Attach`](beryl_card::Effect::Attach): "Attach [an Equipment] to [a creature]",
/// or "attach this Aura to it". The outcome is what moved, if it did.
///
/// [CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b): "If an effect tries to
/// attach an Aura, Equipment, or Fortification to an object or player it can't be attached
/// to, the Aura, Equipment, or Fortification doesn't move. If an effect tries to attach an
/// Aura, Equipment, or Fortification to the object or player it's already attached to, the
/// effect does nothing." An Equipment or a creature that has left the battlefield is not
/// there to attach (see CR 400.7), and an Equipment can't equip more than one creature
/// (see CR 301.5c), so the instruction moves at most one Equipment onto one creature. An Aura moves
/// onto what its enchant abilities allow ([`may_enchant`]).
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    objects: Objects,
    to: Objects,
) -> Outcome<L> {
    let (Some(attached), Some(host)) = (
        run.permanents(objects).slots().sole(),
        run.permanents(to).slots().sole(),
    ) else {
        return Outcome::NOTHING;
    };
    let game = run.game();
    let (s, t) = (&*game.state, game.table);
    let aura = t
        .subtypes()
        .has(Subtype::Aura)
        .of(s.derived(attached).subtypes);
    let may = if aura {
        may_enchant(s, t, attached, host)
    } else {
        may_equip_in(s, t, attached, host)
    };
    if s.object(attached).attached_to() == Some(host) || !may {
        return Outcome::NOTHING;
    }
    let attachment = s.permanent_at(attached).expect("a selected attachment");
    let target = s.permanent_at(host).expect("a selected host");
    game.state.attach(attachment, target);
    Outcome::acted_on(crate::bitboard::Bitboard::single(attached))
}
