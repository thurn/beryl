//! The abilities each permanent was granted, against a reference that finds them from the
//! static abilities on the battlefield and the Effects store, without the table of grants the
//! layers write ([`State::grant_rows`]).
//!
//! Implements: none. Checks the layers' table of grants against a reference calculation.

use beryl_card::{Lends, LentFrom, Zone};

use crate::card::{AbilityKind, Modification, ObjectAbility, Objects};
use crate::filter::Evaluate;
use crate::ids::{CardId, SlotId};
use crate::invariants::{At, Violation};
use crate::layout::{Layout, Queue};
use crate::state::{Characteristics, Flag, GrantRef, LayerChange, LinkKind, Permanents, State};
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
        let Some((mut expected, mut expected_lent)) = reference_grants(s, t, slot) else {
            continue;
        };
        let mut found: Vec<GrantRef<L>> = Vec::new();
        let mut found_lent: Vec<(usize, u8, usize)> = Vec::new();
        for (ability, _) in s.abilities(t, slot) {
            match ability {
                ObjectAbility::Granted(k) => {
                    if let Some(g) = s.granted_ability(slot, k) {
                        found.push(g);
                    }
                }
                ObjectAbility::Lent(..) => {
                    if let Some((card, face, index, _)) = s.lent_ability(t, slot, ability) {
                        found_lent.push((card.index(), face.get(), index.index()));
                    }
                }
                ObjectAbility::Own(_) | ObjectAbility::Merged(..) => {}
            }
        }
        let key = |g: &GrantRef<L>| (g.card.index(), g.grant.index());
        expected.sort_by_key(key);
        found.sort_by_key(key);
        check(
            found == expected,
            "granted abilities differ from the static abilities and effects that grant them",
            Some(At::Slot(slot.index())),
        )?;
        expected_lent.sort_unstable();
        found_lent.sort_unstable();
        check(
            found_lent == expected_lent,
            "lent abilities differ from the static abilities that lend them",
            Some(At::Slot(slot.index())),
        )?;
    }
    Ok(())
}

/// One lent ability's identity: its lending card index, face index and ability index.
type LentKey = (usize, u8, usize);

/// The abilities granted and lent to the permanent in `slot`, in no order: those the exceptions
/// of a copy effect give it, those of each static ability of a permanent on the battlefield that
/// affects it, and those of each effect of a resolved spell or ability on it. `None` where
/// the reference cannot say: a static ability with a condition, one that affects permanents
/// in a way it does not read, or one whose filter reads power, toughness or abilities, which
/// the layers read as layer 6 applies and not as they finally are.
fn reference_grants<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<(Vec<GrantRef<L>>, Vec<LentKey>)> {
    let mut grants: Vec<GrantRef<L>> = if s.object(slot).flags().has(Flag::Copy) {
        s.copy_grants(t, slot).collect()
    } else {
        Vec::new()
    };
    let mut lent: Vec<LentKey> = Vec::new();
    let late = Characteristics::PT
        .union(Characteristics::ADDS_ABILITIES)
        .union(Characteristics::REMOVES_ABILITIES);
    for &from in s.battlefield() {
        let card = s.definition(from).record(t).0;
        for (_, ability) in s.abilities(t, from) {
            let Some(spec) = ability.layer() else {
                continue;
            };
            let has_grant_or_lend = spec
                .modifications
                .iter()
                .any(|m| matches!(m, Modification::Grant(_) | Modification::Lend(_)));
            if !has_grant_or_lend {
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
                | Objects::Convoked
                | Objects::Carried
                | Objects::InZoneOf { .. }
                | Objects::CardsIn { .. }
                | Objects::Paid(_)
                | Objects::Other(_) => return None,
            };
            if affects {
                for &m in spec.modifications {
                    if let Modification::Grant(grant) = m {
                        grants.push(GrantRef { card, grant });
                    } else if let Modification::Lend(spec) = m {
                        collect_lent(s, t, from, you, &words, spec, &mut lent)?;
                    }
                }
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
    Some((grants, lent))
}

/// Appends each ability lent by `spec` from `from` to `out`.
fn collect_lent<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    from: L::Slot,
    you: crate::ids::Seat,
    words: &beryl_card::Words,
    spec: &beryl_card::Lent,
    out: &mut Vec<LentKey>,
) -> Option<()> {
    let (pool, filter) = match spec.from {
        LentFrom::ExiledWithThis(f) => (s.linked(LinkKind::ExiledWith, s.object_ref(from)), f),
        LentFrom::Graveyards(f) => (s.in_zone(Zone::Graveyard), f),
    };
    let matching = pool.filter(|c| filter.matches(s, t, c, you, words));
    let ordered = s.in_game_order(t, matching).ok()?;
    for &c in ordered.as_slice() {
        let (l_card, l_face) = s.definition(c).record(t);
        for (ability, a) in s.abilities(t, c) {
            let ObjectAbility::Own(i) = ability else {
                continue;
            };
            let matches_kind = match spec.which {
                Lends::Activated => matches!(a.kind(), AbilityKind::Activated | AbilityKind::Mana),
            };
            if matches_kind {
                out.push((l_card.index(), l_face.get(), i.index()));
            }
        }
    }
    Some(())
}
