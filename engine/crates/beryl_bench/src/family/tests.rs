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
        let c = control(s, &w.table, &w.in_table).expect("room to recompute");
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

/// The corpora are the benchmark's input, as `rollout::tests::corpus_never_changes` pins the
/// WU corpus: a change that moves one updates this pin on purpose, and its counts move with
/// it.
#[test]
fn family_corpora_never_change() {
    let pins: Vec<(&str, Vec<u16>, u64)> = FAMILIES
        .iter()
        .map(|f| {
            let w = FamilyWorkload::new(f.name);
            let turns = w.positions.iter().map(|s| s.turn().number()).collect();
            (f.name, turns, stable_hash(&w.positions))
        })
        .collect();
    assert_eq!(
        pins,
        vec![
            (
                "blank",
                vec![5, 8, 11, 8, 11, 14, 10, 13, 16, 10, 13, 16],
                7_347_435_361_350_304_198
            ),
            (
                "attachments",
                vec![2, 5, 8, 10, 13, 22, 2, 5, 8, 5, 8, 11],
                5_402_586_628_862_043_351
            ),
            (
                "layers",
                vec![2, 5, 8, 3, 6, 9, 2, 5, 8, 10, 13, 16],
                10_389_015_429_457_374_616
            ),
            (
                "replacement",
                vec![20, 15, 20, 4, 13, 16, 19, 14, 17, 20, 15, 18],
                6_003_108_577_124_308_175
            ),
            (
                "costs",
                vec![2, 5, 8, 13, 16, 19, 2, 5, 8, 11, 14, 17],
                11_457_238_326_421_858_011
            ),
            (
                "zones-and-text",
                vec![3, 6, 9, 5, 8, 11, 2, 5, 8, 5, 13, 16],
                15_369_500_008_821_646_896
            ),
            (
                "copies",
                vec![14, 11, 14, 17, 4, 7, 10, 13, 16, 19, 13, 16],
                17_227_339_049_656_177_457
            ),
            (
                "identity",
                vec![2, 5, 8, 3, 6, 9, 2, 5, 8, 2, 5, 9],
                15_107_076_169_823_050_658
            ),
            (
                "two-faced",
                vec![2, 5, 8, 5, 8, 11, 2, 5, 8, 9, 12, 15],
                11_629_276_330_945_402_876
            ),
            (
                "levels",
                vec![2, 5, 8, 2, 5, 8, 2, 5, 8, 3, 6, 9],
                12_408_771_178_945_865_725
            ),
            (
                "face-down",
                vec![6, 9, 12, 20, 8, 11, 21, 13, 16, 19, 10, 13],
                11_556_153_839_161_840_922
            ),
            (
                "outside",
                vec![6, 9, 12, 13, 16, 19, 2, 5, 8, 14, 17, 20],
                14_260_016_659_534_983_420
            ),
            (
                "turn-structure",
                vec![4, 7, 10, 17, 20, 4, 7, 10, 15, 18, 21, 5],
                2_568_747_076_015_654_357
            ),
            (
                "restart",
                vec![14, 17, 14, 17, 20, 14, 17, 20, 18, 21, 24, 14],
                11_275_274_148_968_723_272
            ),
            (
                "cast-in-resolution",
                vec![2, 5, 8, 3, 6, 9, 2, 5, 8, 2, 5, 8],
                18_398_739_910_328_524_489
            ),
            (
                "combat-assignment",
                vec![3, 6, 9, 2, 5, 8, 2, 5, 8, 14, 17, 3],
                5_759_505_168_609_713_815
            ),
        ]
    );
}

/// Each family pairs nearly all its steps, which `just count-gate --family` needs (it fails
/// past a tenth skipped, `SKIPPED_LIMIT` in `tools/src/beryl_tools/count_gate.py`); the
/// do-nothing family pairs every one; no rollout stalls on a full fixed-capacity list, whose
/// steps would not be what a `Large` game costs, unless a card of the family records the mana
/// spent on an ability's X (`Capability::SpentOnX`, Illusionary Mask): only `Large` holds that
/// record, so every activation for X above 0 stalls a `Small` rollout, and the family is
/// measured up to it; and the counts are the same on a second run,
/// since the gate compares counts from separate runs. The exact counts are not pinned: they
/// move with every change to the engine or the rollout policy, and the corpora themselves are
/// pinned by `family_corpora_never_change`.
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
