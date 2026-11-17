//! Attach ([CR 701.3](https://yawgatog.com/resources/magic-rules/#R7013)).

use beryl_card::{AbilityKind, Objects, TargetKind, TargetSpec};

use crate::card::{Subtype, Types};
use crate::filter::Evaluate;
use crate::game::Game;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::protection;
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
    if aura == host || s.derived(aura).types.contains(Types::CREATURE) {
        return false;
    }
    s.abilities(t, aura)
        .filter(|(_, ability)| ability.kind() == AbilityKind::Spell)
        .flat_map(|(_, ability)| {
            crate::coverage::hit(ability);
            ability.targets()
        })
        .all(|spec| enchantable(s, t, spec, aura, host))
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
        // An Aura that enchants a player, or something on the stack, is not built: the
        // object it is attached to is never one.
        TargetKind::Player
        | TargetKind::Spell
        | TargetKind::Ability
        | TargetKind::ActivatedAbility
        | TargetKind::TriggeredAbility => false,
    };
    in_zone
        && (!spec.put_with_this || s.put_onto_battlefield_with(aura).contains(host))
        && spec
            .filter
            .matches_x(s, t, host, s.controller(aura), &s.words_of(aura), 0)
        && !(zone == crate::state::Zone::Battlefield
            && protection::live(s)
            && protection::protected_from_cold(s, t, host, aura))
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
/// [`may_enchant`] instead.
///
/// [CR 702.16d](https://yawgatog.com/resources/magic-rules/#R70216d): "A permanent with
/// protection can’t be equipped by Equipment that have the stated quality".
pub(crate) fn may_equip<L: Layout>(game: &Game<'_, L>, equipment: L::Slot, host: L::Slot) -> bool {
    may_equip_in(game.state, game.table, equipment, host)
}

/// [`may_equip`], in a state and its table.
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
        && !(protection::live(s) && protection::protected_from_cold(s, t, host, equipment))
}

/// [`Effect::Attach`](beryl_card::Effect::Attach): "Attach [an Equipment] to [a creature]",
/// or "attach this Aura to it". The outcome is what moved, if it did.
///
/// [CR 701.3b](https://yawgatog.com/resources/magic-rules/#R7013b): "If an effect tries to
/// attach an Aura, Equipment, or Fortification to an object or player it can't be attached
/// to, the Aura, Equipment, or Fortification doesn't move. If an effect tries to attach an
/// Aura, Equipment, or Fortification to the object or player it's already attached to, the
/// effect does nothing." An Equipment or a creature that has left the battlefield is not
/// there to attach (CR 400.7), and an Equipment can't equip more than one creature (CR
/// 301.5c), so the instruction moves at most one Equipment onto one creature. An Aura moves
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
        may_equip(game, attached, host)
    };
    if game.state.object(attached).attached_to() == Some(host) || !may {
        return Outcome::NOTHING;
    }
    let attachment = s.permanent_at(attached).expect("a selected attachment");
    let target = s.permanent_at(host).expect("a selected host");
    game.state.attach(attachment, target);
    Outcome::acted_on(crate::bitboard::Bitboard::single(attached))
}
