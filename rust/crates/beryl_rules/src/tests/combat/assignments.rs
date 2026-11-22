//! Damage assignments with a blocker of several attackers or changing strike abilities.

use super::{BEAR, CardTable, DecisionKind, Game, Opt, Slot, TurnStep, options, stop_at};
use crate::GameTable;
use crate::card::{Ability, CardDef, Face, Keywords, Objects, Players, RuleChange, Types};
use crate::fixed::Full;
use crate::ids::SlotId as _;
use crate::layout::Small;
use crate::replacement::event::Sink;
use crate::state::{Continuous, Duration, LayerChange};
use beryl_card::{Governed, When};
use beryl_kernel::format::Format;

static ANY_NUMBER: RuleChange = RuleChange {
    players: Players::You,
    what: Governed::BlockAnyNumber(Objects::This),
    when: When::AsUsual,
};

static GUARD: CardDef = CardDef {
    name: "Test Guard",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(6),
        abilities: &[Ability::rule_change(&ANY_NUMBER)],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static WEAK: CardDef = CardDef {
    name: "Test Weak",
    shape: beryl_card::definition::Shape::Single,
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&BEAR, &GUARD, &WEAK]);

fn guard_game() -> Game {
    let deck = &[(14, "Bear"), (4, "Test Guard"), (2, "Test Weak")];
    let t = GameTable::new(Format::CONSTRUCTED, &CARDS, [deck, deck]).unwrap();
    Game::with_table(t, 1)
}

#[test]
fn a_blocker_of_two_attacks_divides_for_its_controller() {
    let mut g = guard_game();
    let (a, d) = (g.attacker, g.defender());
    let first = g.put(a, "Bear");
    let second = g.put(a, "Test Weak");
    let guard = g.put(d, "Test Guard");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(first));
    g.pick(DecisionKind::Attacker, Opt::Attack(second));
    g.pick(DecisionKind::Blocker, Opt::Block(guard));
    g.pick_object(DecisionKind::BlockTarget, first);
    g.pick(DecisionKind::Blocker, Opt::Block(guard));

    let divide = g.expect(DecisionKind::DamageDivision);
    assert_eq!(
        (divide.player, divide.decider, divide.about.sole()),
        (d, d, Some(guard))
    );
    assert_eq!(crate::combat::assigner(&g.s, first), a);
    assert_eq!(crate::combat::assigner(&g.s, guard), d);
    assert!(options(&divide).contains(&Opt::Lethal(second)));
    g.pick(DecisionKind::DamageDivision, Opt::Lethal(second));
    g.finish_combat();

    assert!(g.on_battlefield(first));
    assert!(!g.on_battlefield(second));
    assert_eq!(g.s.object(guard).damage(), 3);
    assert_eq!(g.life(d), 20);
}

#[test]
fn the_fourth_block_fits_after_widening() {
    let mut g = guard_game();
    let card_slots = g.cards(g.attacker, "Bear");
    let attackers: Vec<_> = card_slots[..4]
        .iter()
        .map(|&slot| g.put_slot(slot))
        .collect();
    let guard = g.put(g.defender(), "Test Guard");
    let defender = g.defender();
    let blocker = g.s.permanent_at(guard).unwrap();
    g.s.declare_blocker(blocker);

    for (index, slot) in attackers.into_iter().enumerate() {
        let attacker = g.s.permanent_at(slot).unwrap();
        g.s.declare_attacker(attacker, defender);
        if index < 3 {
            g.s.block(blocker, attacker).unwrap();
            continue;
        }
        assert_eq!(g.s.block(blocker, attacker), Err(Full));
        assert!(!g.s.object(slot).is_blocked());
        let mut large = g.s.widen();
        let blocker = large.permanent_at(guard.to().unwrap()).unwrap();
        let attacker = large.permanent_at(slot.to().unwrap()).unwrap();
        large.block(blocker, attacker).unwrap();
        assert_eq!(
            large.object(guard.to().unwrap()).blocked_attackers().len(),
            4
        );
        assert!(large.object(slot.to().unwrap()).is_blocked());
        assert!(large.try_relayout::<Small>(&g.t).is_err());
    }
}

fn change_keywords(g: &mut Game, slot: Slot, change: LayerChange) {
    let (card, _) = g.s.copiable(slot).record(&g.t);
    let effect = Continuous::Layer(g.s.object_ref(slot), change);
    g.s.start_effect(card, effect, Duration::Indefinite)
        .unwrap();
    crate::continuous::layers::refresh(&mut g.s, &g.t, &mut Sink::off()).unwrap();
}

#[test]
fn regular_damage_uses_first_step_history_and_current_double_strike() {
    for (starts_with, changes_to, life_after_second) in [
        ("Knight", Keywords::DOUBLE_STRIKE, 13),
        ("Bear", Keywords::FIRST_STRIKE, 15),
        ("Bear", Keywords::NONE, 15),
    ] {
        let mut g = Game::new();
        let (a, d) = (g.attacker, g.defender());
        let striker = g.put(a, starts_with);
        let regular = g.put(a, "Ogre");
        if starts_with == "Bear" {
            change_keywords(
                &mut g,
                striker,
                LayerChange::AddKeywords(Keywords::DOUBLE_STRIKE),
            );
        }
        g.pass_to_combat();
        g.pick(DecisionKind::Attacker, Opt::Attack(striker));
        g.pick(DecisionKind::Attacker, Opt::Attack(regular));
        stop_at(&mut g, TurnStep::FirstStrikeDamage);
        assert_eq!(g.life(d), 18);

        change_keywords(&mut g, striker, LayerChange::LoseAllAbilities);
        change_keywords(&mut g, striker, LayerChange::AddKeywords(changes_to));
        change_keywords(
            &mut g,
            regular,
            LayerChange::AddKeywords(Keywords::FIRST_STRIKE),
        );
        g.finish_combat();
        assert_eq!(g.life(d), life_after_second);
    }
}
