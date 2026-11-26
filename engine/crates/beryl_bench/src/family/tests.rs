use super::*;
use beryl_engine::table::Capability;

use crate::rollout::stable_hash;

/// The control takes every card of the family out of play and leaves the rest alone: each
/// permanent of the family is its stand-in, under the same slot, and no effect of a card of the
/// family is left.
#[test]
fn control_takes_the_family_out() {
    let w = FamilyWorkload::new("attachments");
    for s in &w.positions {
        assert!(w.in_table.in_play(s));
        let c = control(s, &w.table, &w.control_table, &w.in_table).expect("room to recompute");
        assert!(!w.in_table.in_play(&c));
        let slots = |s: &State| {
            s.permanents_with_derived()
                .map(|(slot, _)| slot)
                .collect::<Vec<_>>()
        };
        assert_eq!(slots(s), slots(&c));
        assert_eq!(s.stack().len(), c.stack().len());
        assert_eq!(s.turn().number(), c.turn().number());
    }
}

/// A stand-in is its card without its static and triggered abilities and keywords, and keeps
/// what it is cast and activated as.
#[test]
fn a_stand_in_keeps_spell_and_activated_abilities() {
    let scimitar = CARDS.get(names::LEONIN_SCIMITAR).expect("Leonin Scimitar");
    let blank = stand_in(scimitar);
    assert_eq!(blank.name, scimitar.name);
    let kinds = |def: &CardDef| -> Vec<AbilityKind> {
        def.front()
            .abilities
            .iter()
            .map(beryl_engine::card::Ability::kind)
            .collect()
    };
    assert_eq!(kinds(blank), vec![AbilityKind::Activated]);
    assert_eq!(blank.front().types, scimitar.front().types);
    assert_eq!(blank.front().subtypes, scimitar.front().subtypes);
    let tundra = CARDS.get(names::TUNDRA).expect("Tundra");
    assert_eq!(kinds(stand_in(tundra)), kinds(tundra));
}

/// The count gate compares counts from separate runs, so a family's corpus must be the same
/// every time it is built, and it must be full. The corpus is cut from games the engine plays,
/// so its exact states move with every engine change that alters play: they are not pinned
/// (a pin failed after unrelated engine changes and was only ever updated to match).
#[test]
fn family_corpora_are_deterministic_and_full() {
    for f in FAMILIES {
        let a = FamilyWorkload::new(f.name);
        let b = FamilyWorkload::new(f.name);
        assert_eq!(a.positions.len(), POSITIONS, "{}: a short corpus", f.name);
        assert_eq!(
            stable_hash(&a.positions),
            stable_hash(&b.positions),
            "{}: two builds differ",
            f.name
        );
    }
}

/// Each family pairs nearly all its steps, which `just count-gate --family` needs (it fails
/// past a tenth skipped, `SKIPPED_LIMIT` in `tools/src/beryl_tools/count_gate.py`); the
/// do-nothing family pairs every one; no rollout stalls on a full fixed-capacity list, whose
/// steps would not be what a `Large` game costs, unless a card of the family records the mana
/// spent on an ability's X (`Capability::SpentOnX`, Illusionary Mask): only `Large` holds that
/// record, so every activation for X above 0 stalls a `Small` rollout, and the family is
/// measured up to it; the control's steps keep the engine's invariants on the control's table,
/// which `roll_paired` checks in a debug build; and the counts are the same on a second run,
/// since the gate compares counts from separate runs. The exact counts are not pinned: they
/// move with every change to the engine or the rollout policy, and the corpora are checked by
/// `family_corpora_are_deterministic_and_full`.
#[test]
fn families_pair_their_steps() {
    for f in FAMILIES {
        let w = FamilyWorkload::new(f.name);
        let p = roll_paired(&w);
        assert!(p.paired > 0, "{}: {p:?}", f.name);
        assert!(
            p.skipped * 10 <= p.paired + p.skipped,
            "{}: over a tenth of the steps skipped: {p:?}",
            f.name
        );
        if f.name == "blank" {
            assert_eq!(p.skipped, 0, "{p:?}");
        }
        if !w.table.can(Capability::SpentOnX) {
            assert_eq!(p.capacity_stalls, 0, "{}: {p:?}", f.name);
        }
        assert_eq!(roll_paired(&w), p, "{}: a second run differs", f.name);
    }
}

/// The control runs on a table without the capabilities the family's cards switch on, so its
/// steps skip the paths they pay for: banding's combat for the combat-assignment family, the
/// turn plan for the turn-structure family. The present table keeps them.
#[test]
fn the_control_table_lacks_the_family_capabilities() {
    for (name, capability) in [
        ("combat-assignment", Capability::Banding),
        ("turn-structure", Capability::TurnStructure),
    ] {
        let (t, in_table) = InTable::table(family(name).expect("a family"));
        assert!(t.can(capability), "{name}: {capability:?}");
        assert!(
            !control_table(&t, &in_table).can(capability),
            "{name}: {capability:?}"
        );
    }
}
