//! The abilities each permanent was granted, against a reference that finds them from the
//! static abilities on the battlefield and the Effects store, without the table of grants the
//! layers write ([`State::grant_rows`]).
//!
//! Implements: none. Checks the layers' table of grants against a reference calculation.

use crate::card::{Modification, ObjectAbility, Objects};
use crate::filter::Evaluate;
use crate::ids::{CardId, SlotId};
use crate::invariants::{At, Violation};
use crate::layout::Layout;
use crate::state::{Characteristics, Flag, GrantRef, LayerChange, Permanents, State};
use crate::table::{Capability, GameTable};

use super::check;

/// The abilities other objects granted each permanent ([`State::abilities`]), as a multiset,
/// against those the reference finds for it ([`reference_grants`]). Checked in a game with a
/// card that grants an ability, where no permanent has lost an ability and no text has
/// changed: a removal makes the order of layer 6 matter, which a multiset does not see.
pub(super) fn check_grants<L: Layout>(s: &State<L>, t: &GameTable) -> Result<(), Violation> {
    if !t.can(Capability::Grants)
        || s.any_abilities_lost()
        || s.changes_text()
        || t.can(Capability::Merges)
        || t.can(Capability::GrantsLayerEffect)
    {
        return Ok(());
    }
    for &slot in s.battlefield() {
        let Some(mut expected) = reference_grants(s, t, slot) else {
            continue;
        };
        let mut found: Vec<GrantRef<L>> = (s.abilities(t, slot))
            .filter_map(|(ability, _)| match ability {
                ObjectAbility::Granted(k) => s.granted_ability(slot, k),
                ObjectAbility::Own(_) | ObjectAbility::Merged(..) => None,
            })
            .collect();
        let key = |g: &GrantRef<L>| (g.card.index(), g.grant.index());
        expected.sort_by_key(key);
        found.sort_by_key(key);
        check(
            found == expected,
            "granted abilities differ from the static abilities and effects that grant them",
            Some(At::Slot(slot.index())),
        )?;
    }
    Ok(())
}

/// The abilities granted to the permanent in `slot`, in no order: those the exceptions of a
/// copy effect give it, those of each static ability of a permanent on the battlefield that
/// affects it, and those of each effect of a resolved spell or ability on it. `None` where
/// the reference cannot say: a static ability with a condition, one that affects permanents
/// in a way it does not read, or one whose filter reads power, toughness or abilities, which
/// the layers read as layer 6 applies and not as they finally are.
fn reference_grants<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<Vec<GrantRef<L>>> {
    let mut grants: Vec<GrantRef<L>> = if s.object(slot).flags().has(Flag::Copy) {
        s.copy_grants(t, slot).collect()
    } else {
        Vec::new()
    };
    let late = Characteristics::PT
        .union(Characteristics::ADDS_ABILITIES)
        .union(Characteristics::REMOVES_ABILITIES);
    for &from in s.battlefield() {
        let card = s.definition(from).record(t).0;
        for (_, ability) in s.abilities(t, from) {
            let Some(spec) = ability.layer() else {
                continue;
            };
            let granted = (spec.modifications.iter()).filter_map(|m| {
                if let Modification::Grant(grant) = *m {
                    Some(grant)
                } else {
                    None
                }
            });
            let mut granted = granted.peekable();
            if granted.peek().is_none() {
                continue;
            }
            if spec.condition.is_some() {
                return None;
            }
            let you = s.controller(from);
            let words = s.words_of(from);
            let affects = match spec.affected {
                Objects::This => from == slot,
                Objects::AttachedTo => s.object(from).attached_to() == Some(slot),
                Objects::All { filter, .. } | Objects::Everywhere(filter) => {
                    if crate::continuous::layers::filter_reads(filter).intersects(late) {
                        return None;
                    }
                    filter.matches(s, t, slot, you, &words)
                }
                Objects::Other(&Objects::All { filter, .. }) => {
                    if crate::continuous::layers::filter_reads(filter).intersects(late) {
                        return None;
                    }
                    from != slot && filter.matches(s, t, slot, you, &words)
                }
                Objects::Target(_)
                | Objects::Those
                | Objects::TriggeredOn
                | Objects::TriggeredOnIn(_)
                | Objects::ExiledWithThis
                | Objects::LibraryTop { .. }
                | Objects::ExceptThose(_)
                | Objects::PutOntoBattlefieldWithThis
                | Objects::Carried
                | Objects::InZoneOf { .. }
                | Objects::CardsIn { .. }
                | Objects::Paid(_)
                | Objects::Other(_) => return None,
            };
            if affects {
                grants.extend(granted.map(|grant| GrantRef { card, grant }));
            }
        }
    }
    for (e, on, change) in s.layer_effects() {
        let LayerChange::Grant(grant) = change else {
            continue;
        };
        let affects = match on {
            Permanents::One(object) => object.slot == slot && s.is_current(object),
            Permanents::Set(set) => s.locked_in(set).contains(slot),
        };
        if affects {
            grants.push(GrantRef {
                card: e.source,
                grant,
            });
        }
    }
    Some(grants)
}
