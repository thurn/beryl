//! Trample, lifelink, indestructible and landwalk (CR 702.19, 702.15, 702.12, 702.14), with
//! the test-only creatures of the combat tests.

use super::{Game, options, stop_at};
use crate::actions::destroy::destroy;
use crate::event::Sink;
use crate::state::Zone;
use crate::zone::Place;
use crate::{DecisionKind, Opt, TurnStep};

#[test]
fn trample_assigns_lethal_damage_to_a_lone_blocker_and_the_rest_to_the_player() {
    // A 5/5 trampler blocked by a 2/2: 2 to the Bear, 3 to the player, and no division to
    // ask about.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let wurm = g.put(a, "Wurm");
    let bear = g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(wurm));
    g.pick(DecisionKind::Blocker, Opt::Block(bear));
    g.finish_combat();
    assert!(!g.on_battlefield(bear));
    assert_eq!(g.life(d), 17);
    assert_eq!(g.s.object(wurm).damage(), 2);
}

#[test]
fn trample_reaches_the_player_only_once_every_blocker_is_dealt_lethal_damage() {
    // A 5/5 trampler blocked by two 2/2s must kill one, a pick made for its controller as the
    // only one. Killing both leaves 1 for the player; stopping after one puts the rest on
    // that one.
    for both in [false, true] {
        let mut g = Game::new();
        let (a, d) = (g.attacker, g.defender());
        let wurm = g.put(a, "Wurm");
        let x = g.put(d, "Bear");
        let y = g.put(d, "Bear");
        g.pass_to_combat();
        g.pick(DecisionKind::Attacker, Opt::Attack(wurm));
        g.pick(DecisionKind::Blocker, Opt::Block(x.min(y)));
        g.pick(DecisionKind::Blocker, Opt::Block(x.max(y)));
        let division = g.expect(DecisionKind::DamageDivision);
        let [Opt::Done, Opt::Lethal(other)] = options(&division)[..] else {
            panic!("{division:?}");
        };
        // With both picked, "done" is the only option left, and it is taken for them.
        let pick = if both { Opt::Lethal(other) } else { Opt::Done };
        g.pick(DecisionKind::DamageDivision, pick);
        g.finish_combat();
        assert_eq!(g.on_battlefield(other), !both);
        assert!(!g.on_battlefield(if other == x { y } else { x }));
        assert_eq!(g.life(d), if both { 19 } else { 20 });
    }
}

#[test]
fn a_trampler_whose_blockers_all_left_assigns_its_damage_to_the_player() {
    // CR 702.19d: with no creature blocking it when damage is assigned, a blocked trampler's
    // damage goes to the player, where another blocked attacker's goes nowhere.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let wurm = g.put(a, "Wurm");
    let bear = g.put(d, "Bear");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(wurm));
    g.pick(DecisionKind::Blocker, Opt::Block(bear));
    stop_at(&mut g, TurnStep::DeclareBlockers);
    g.s.move_object(&mut Sink::off(), bear, Zone::Graveyard, Place::Top)
        .unwrap();
    g.finish_combat();
    assert_eq!(g.life(d), 15);
}

#[test]
fn lifelink_gains_its_controller_the_damage_it_deals() {
    // Unblocked, the Cleric's 2 damage to the player gains its controller 2 life; blocked, its
    // damage to the Bear does the same.
    for blocked in [false, true] {
        let mut g = Game::new();
        let (a, d) = (g.attacker, g.defender());
        let cleric = g.put(a, "Cleric");
        let bear = g.put(d, "Bear");
        g.pass_to_combat();
        g.pick(DecisionKind::Attacker, Opt::Attack(cleric));
        if blocked {
            g.pick(DecisionKind::Blocker, Opt::Block(bear));
        } else {
            g.pick(DecisionKind::Blocker, Opt::Done);
        }
        g.finish_combat();
        assert_eq!(g.life(a), 22);
        assert_eq!(g.life(d), if blocked { 20 } else { 18 });
        assert_eq!(g.on_battlefield(bear), !blocked);
    }
}

#[test]
fn indestructible_survives_lethal_damage_and_destruction() {
    // The Golem blocks a 4/4 and keeps 4 damage on it; an effect that destroys it does
    // nothing.
    let mut g = Game::new();
    let (a, d) = (g.attacker, g.defender());
    let giant = g.put(a, "Giant");
    let golem = g.put(d, "Golem");
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::Attack(giant));
    g.pick(DecisionKind::Blocker, Opt::Block(golem));
    g.finish_combat();
    assert!(g.on_battlefield(golem));
    assert_eq!(g.s.object(golem).damage(), 4);
    let golem_ref = g.s.object_ref(golem);
    destroy(
        &mut crate::game::Game::new(&mut g.s, &g.t, &mut Sink::off()),
        golem_ref,
    )
    .unwrap();
    assert!(g.on_battlefield(golem));
}

#[test]
fn landwalk_is_unblockable_only_while_the_defender_controls_that_land_type() {
    // The Wraith has swampwalk. Without a Swamp the defender blocks it with the Bear; with
    // one, the defender is not asked, and a Swamp of the attacker's changes nothing.
    for swamp in [false, true] {
        let mut g = Game::new();
        let (a, d) = (g.attacker, g.defender());
        let wraith = g.put(a, "Wraith");
        g.put(a, "Swamp");
        let bear = g.put(d, "Bear");
        if swamp {
            g.put(d, "Swamp");
        }
        g.pass_to_combat();
        g.pick(DecisionKind::Attacker, Opt::Attack(wraith));
        if !swamp {
            g.pick(DecisionKind::Blocker, Opt::Block(bear));
        }
        g.finish_combat();
        assert_eq!(g.life(d), if swamp { 17 } else { 20 });
        assert_eq!(g.on_battlefield(bear), swamp);
    }
}
