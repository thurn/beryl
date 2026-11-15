use super::*;
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
        let slots = |s: &State| s.permanents().map(|(slot, _)| slot).collect::<Vec<_>>();
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
                9_298_369_477_611_768_779
            ),
            (
                "attachments",
                vec![2, 5, 8, 10, 13, 22, 2, 5, 8, 5, 8, 11],
                12_916_577_998_348_938_393
            ),
            (
                "layers",
                vec![2, 5, 8, 3, 6, 9, 2, 5, 8, 10, 13, 16],
                15_055_249_918_153_319_555
            ),
            (
                "replacement",
                vec![20, 15, 20, 4, 13, 16, 19, 14, 17, 20, 15, 18],
                3_337_079_432_780_671_536
            ),
            (
                "costs",
                vec![2, 5, 8, 13, 16, 19, 2, 5, 8, 11, 14, 17],
                16_420_930_836_580_429_720
            ),
            (
                "zones-and-text",
                vec![3, 6, 9, 5, 8, 11, 2, 5, 8, 5, 13, 16],
                15_506_361_981_812_984_104
            ),
            (
                "copies",
                vec![14, 11, 14, 17, 4, 7, 10, 13, 16, 19, 13, 16],
                11_195_901_546_338_064_319
            ),
        ]
    );
}

/// Each family pairs nearly all its steps, which the gate needs (it fails past a tenth
/// skipped), and the do-nothing family pairs every one.
#[test]
fn families_pair_their_steps() {
    let found: Vec<(&str, Pairing)> = FAMILIES
        .iter()
        .map(|f| (f.name, roll_paired(&FamilyWorkload::new(f.name))))
        .collect();
    assert_eq!(
        found,
        vec![
            (
                "blank",
                Pairing {
                    paired: 3_695,
                    skipped: 0,
                    capacity_stalls: 0,
                    layer_recomputes: 1_439,
                    transactions: 0
                }
            ),
            (
                "attachments",
                Pairing {
                    paired: 3_619,
                    skipped: 30,
                    capacity_stalls: 0,
                    layer_recomputes: 821,
                    transactions: 3_136
                }
            ),
            (
                "layers",
                Pairing {
                    paired: 1_700,
                    skipped: 0,
                    capacity_stalls: 0,
                    layer_recomputes: 880,
                    transactions: 0
                }
            ),
            (
                "replacement",
                Pairing {
                    paired: 2_775,
                    skipped: 0,
                    capacity_stalls: 0,
                    layer_recomputes: 827,
                    transactions: 7_700
                }
            ),
            (
                "costs",
                Pairing {
                    paired: 2_953,
                    skipped: 0,
                    capacity_stalls: 0,
                    layer_recomputes: 1_278,
                    transactions: 2_901
                }
            ),
            (
                "zones-and-text",
                Pairing {
                    paired: 2_662,
                    skipped: 13,
                    capacity_stalls: 0,
                    layer_recomputes: 1_097,
                    transactions: 2_515
                }
            ),
            (
                "copies",
                Pairing {
                    paired: 3_774,
                    skipped: 153,
                    capacity_stalls: 0,
                    layer_recomputes: 1_252,
                    transactions: 3_771
                }
            ),
        ]
    );
}
