//! Paying life as a cost ([CR 119.4]): "If a cost or effect allows a player to pay an amount of
//! life greater than 0, the player may do so only if their life total is greater than or equal
//! to the amount of the payment. If a player pays life, the payment is subtracted from their
//! life total; in other words, the player loses that much life." And [CR 119.4b]: "Players can
//! always pay 0 life, no matter what their (or their team's) life total is, and even if an
//! effect says players can't pay life."
//!
//! [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
//! [CR 119.4b]: https://yawgatog.com/resources/magic-rules/#R1194b

use beryl_card::effect::build::{draw, you};
use beryl_card::{Cost, CostPart};

use super::LAND;
use super::cast::{Game, index_of};
use crate::card::{Ability, CardDef, Face, ObjectAbility, Types};
use crate::event::Event;
use crate::state::Zone;
use crate::{
    CardTable, DecisionKind, Decklist, Opt, Outcome, Profile, Replay, Scratch, Step, next,
};

/// "Pay 3 life: Draw a card."
static BLOODLETTER: CardDef = CardDef {
    name: "Test Bloodletter",
    tokens: &[],
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[Ability::activated(Cost::part(&CostPart::Life(3))).effect(&draw(you(), 1))],
        ..Face::EMPTY
    }],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &BLOODLETTER]);
const DECK: &Decklist = &[(30, "Test Land"), (10, "Test Bloodletter")];

/// A game whose starting player has a Test Bloodletter on the battlefield and `life` life, at
/// their precombat main phase, and the option that activates its ability.
fn bloodletter_at(life: i16) -> (Game, Opt) {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let slot = g.give(g.me, "Test Bloodletter", Zone::Battlefield);
    g.s.board().set_life(g.me, life);
    let activate = Opt::Activate {
        slot,
        ability: ObjectAbility::own(0),
    };
    (g, activate)
}

#[test]
fn a_life_total_below_the_payment_cannot_pay_it() {
    let (mut g, activate) = bloodletter_at(2);
    let d = g.current();
    assert_eq!(d.kind, DecisionKind::Priority);
    assert!(index_of(&d, activate).is_none(), "2 life cannot pay 3");
}

/// A life total equal to the payment pays it, and the payment is a life loss like any other:
/// it is reported as one, and the player at 0 life loses the game when state-based actions are
/// next checked ([CR 704.5a]): "If a player has 0 or less life, that player loses the game."
///
/// [CR 704.5a]: https://yawgatog.com/resources/magic-rules/#R7045a
#[test]
fn a_life_total_equal_to_the_payment_pays_it_as_a_loss_of_life() {
    let (mut g, activate) = bloodletter_at(3);
    g.x = Scratch::recording();
    g.pick(activate);
    assert_eq!(g.s.player(g.me).life(), 0);
    let lost: Vec<_> = (g.x.events.drain())
        .filter_map(|e| match e {
            Event::LifeLoss { player, amount } => Some((player, amount)),
            _ => None,
        })
        .collect();
    assert_eq!(lost, [(g.me, 3)]);
    let step = next(&mut g.s, &g.t, &mut g.x, &mut Replay);
    assert!(matches!(step, Step::Over(Outcome::Win(winner)) if winner == g.them));
}

#[test]
fn any_life_total_can_pay_0_life() {
    let (mut g, _) = bloodletter_at(-2);
    assert!(g.s.can_pay_life(g.me, 0));
    assert!(!g.s.can_pay_life(g.me, 1));
    g.s.board().set_life(g.me, 1);
    assert!(g.s.can_pay_life(g.me, 1));
}
