//! One pick that declares every interchangeable attacker (`Opt::AttackAll`), offered to a
//! seat with `Profile::group_picks`: it leaves the state and the events the single picks
//! leave, and is offered only where those single picks would ask nothing in between.

use super::{BEAR, Game, OGRE, WALKER, options};
use crate::ids::{Seat, Slot};
use crate::{CardTable, DecisionKind, Decklist, GameTable, Opt, Profile};
use beryl_kernel::format::Format;

static SWARM: CardTable = CardTable::new(&[&BEAR, &OGRE, &WALKER]);

/// A game whose players' flags are `flags`, with `bears` Bears and an Ogre on the attacking
/// player's battlefield, put there lowest slot first so that battlefield order is slot order,
/// the Bears' slots in that order, and the Ogre's.
fn board(seed: u64, bears: usize, flags: impl Fn(Seat) -> bool) -> (Game, Vec<Slot>, Slot) {
    const DECK: &Decklist = &[(52, "Bear"), (4, "Ogre"), (4, "Walker")];
    let t = GameTable::new(Format::CONSTRUCTED, &SWARM, [DECK, DECK]).unwrap();
    let mut g = Game::with_table(t, seed);
    let a = g.attacker;
    let bears = (0..bears).map(|_| g.put(a, "Bear")).collect();
    let ogre = g.put(a, "Ogre");
    set_flags(&mut g, flags);
    (g, bears, ogre)
}

fn set_flags(g: &mut Game, flags: impl Fn(Seat) -> bool) {
    g.s.set_profile(Profile {
        group_picks: Seat::BOTH.map(&flags),
        ..g.s.profile()
    });
}

/// Whether the attack declaration open now offers `Opt::AttackAll`.
fn offers_attack_all(g: &mut Game) -> bool {
    let d = g.expect(DecisionKind::Attacker);
    options(&d).iter().any(|o| matches!(o, Opt::AttackAll(_)))
}

#[test]
fn attacking_with_all_leaves_what_the_single_picks_leave() {
    let (mut all, bears, _) = board(3, 5, |_| true);
    let (mut single, _, _) = board(3, 5, |_| true);
    all.pass_to_combat();
    single.pass_to_combat();
    all.pick(DecisionKind::Attacker, Opt::AttackAll(bears[0]));
    // The Bears were put onto the battlefield lowest slot first, so the lowest Bear not yet
    // attacking, the one the declaration offers, is the next in battlefield order.
    for &bear in &bears {
        single.pick(DecisionKind::Attacker, Opt::attack(bear));
    }
    let left = |g: &mut Game| options(&g.expect(DecisionKind::Attacker));
    assert_eq!(left(&mut all), left(&mut single));
    all.pick(DecisionKind::Attacker, Opt::Done);
    single.pick(DecisionKind::Attacker, Opt::Done);
    all.finish_combat();
    single.finish_combat();
    assert!(bears.iter().all(|&b| all.s.object(b).is_tapped()));
    assert_eq!(all.life(all.defender()), 10);
    // The one pick is fewer decisions offered this turn than the five it stands for, and the
    // count of them is all that differs.
    let fewer = single.s.budgets().decisions - all.s.budgets().decisions;
    assert_eq!(fewer, 4);
    for _ in 0..fewer {
        all.s.count_decision();
    }
    assert_eq!(all.x.events.events(), single.x.events.events());
    assert_eq!(all.s, single.s);
}

#[test]
fn attacking_with_all_is_offered_only_to_a_flagged_seat_with_a_class_and_one_defender() {
    // Offered: the attacking seat's flag, five Bears, nothing to attack but the player.
    let (mut g, bears, ogre) = board(1, 5, |_| true);
    g.pass_to_combat();
    let d = g.expect(DecisionKind::Attacker);
    assert_eq!(
        options(&d),
        vec![
            Opt::Done,
            Opt::attack(bears[0]),
            Opt::AttackAll(bears[0]),
            Opt::attack(ogre),
        ]
    );

    // The flags off.
    let (mut g, _, _) = board(1, 5, |_| false);
    g.pass_to_combat();
    assert!(!offers_attack_all(&mut g));

    // A class of one.
    let (mut g, _, _) = board(1, 1, |_| true);
    g.pass_to_combat();
    assert!(!offers_attack_all(&mut g));

    // The defending player controls a planeswalker, so each attacker's target is its own.
    let (mut g, _, _) = board(1, 5, |_| true);
    let d = g.defender();
    g.put(d, "Walker");
    g.pass_to_combat();
    assert!(!offers_attack_all(&mut g));

    // Only the defending seat has the flag.
    let (mut g, _, _) = board(1, 5, |_| false);
    let defender = g.defender();
    set_flags(&mut g, |seat| seat == defender);
    g.pass_to_combat();
    assert!(!offers_attack_all(&mut g));
}

#[test]
fn sixty_interchangeable_attackers_are_declared_in_one_decision() {
    // A library holds at most 60 cards in the small layout.
    const DECK: &Decklist = &[(60, "Bear")];
    let t = GameTable::new(Format::CONSTRUCTED, &SWARM, [DECK, DECK]).unwrap();
    let mut g = Game::with_table(t, 1);
    let a = g.attacker;
    let bears: Vec<Slot> = g.cards(a, "Bear");
    assert_eq!(bears.len(), 60);
    for &bear in &bears {
        g.put_slot(bear);
    }
    set_flags(&mut g, |_| true);
    g.pass_to_combat();
    g.pick(DecisionKind::Attacker, Opt::AttackAll(bears[0]));
    assert!(bears.iter().all(|&b| g.s.object(b).is_attacking()));
    // With every creature attacking, finishing the declaration is the one option left, which
    // is taken silently (§7.2), and the defender has no creature to block with: the 120
    // damage ends the game without another decision.
    assert_eq!(
        crate::next(&mut g.s, &g.t, &mut g.x, &mut crate::Replay),
        crate::Step::Over(crate::Outcome::Win(a))
    );
}
