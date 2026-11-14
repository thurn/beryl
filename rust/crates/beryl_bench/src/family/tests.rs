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
                11_778_248_432_486_834_595
            ),
            (
                "attachments",
                vec![2, 5, 8, 10, 13, 22, 2, 5, 8, 5, 8, 11],
                12_596_629_640_630_511_588
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
                    skipped: 0
                }
            ),
            (
                "attachments",
                Pairing {
                    paired: 3_619,
                    skipped: 16
                }
            ),
        ]
    );
}
