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
                12_791_894_566_829_319_380
            ),
            (
                "replacement",
                vec![20, 15, 20, 4, 13, 16, 19, 14, 17, 20, 15, 18],
                10_501_567_743_470_367_679
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
                994_241_158_328_713_239
            ),
            (
                "identity",
                vec![2, 5, 8, 3, 6, 9, 2, 5, 8, 2, 5, 9],
                5_480_715_648_577_068_636
            ),
            (
                "two-faced",
                vec![2, 5, 8, 5, 8, 11, 2, 5, 8, 9, 12, 15],
                3_746_690_392_626_505_240
            ),
            (
                "levels",
                vec![2, 5, 8, 2, 5, 8, 2, 5, 8, 3, 6, 9],
                3_124_586_745_196_377_747
            ),
            (
                "face-down",
                vec![4, 21, 24, 3, 13, 16, 19, 6, 9, 12, 4, 6],
                9_899_035_086_967_694_136
            ),
            (
                "outside",
                vec![6, 9, 12, 13, 16, 19, 2, 5, 8, 14, 17, 20],
                3_301_674_592_042_243_926
            ),
        ]
    );
}

/// Each family pairs nearly all its steps, which `just count-gate --family` needs (it fails
/// past a tenth skipped, `SKIPPED_LIMIT` in `tools/src/beryl_tools/count_gate.py`); the
/// do-nothing family pairs every one; no rollout stalls on a full fixed-capacity list, whose
/// steps would not be what a `Large` game costs; and the counts are the same on a second run,
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
        assert_eq!(p.capacity_stalls, 0, "{}: {p:?}", f.name);
        assert_eq!(roll_paired(&w), p, "{}: a second run differs", f.name);
    }
}
