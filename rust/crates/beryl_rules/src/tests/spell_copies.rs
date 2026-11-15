//! Storm (CR 702.40) far past what a scenario casts, shown with a test card.

use beryl_card::TargetSpec;
use beryl_card::effect::build::{gain_life, target_player};

use super::LAND;
use super::cast::{Game, cast};
use crate::card::{Ability, CardDef, Face, Types};
use crate::state::{Count, Zone};
use crate::{CardTable, Choice, DecisionKind, Decklist, Opt, Profile};

/// "Target player gains 1 life. Storm"
static SURGE: CardDef = CardDef {
    name: "Test Surge",
    faces: &[Face {
        types: Types::INSTANT,
        abilities: &[
            Ability::spell()
                .target(&TargetSpec::player())
                .effect(&gain_life(target_player(), 1)),
            Ability::storm(),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&LAND, &SURGE]);
const DECK: &Decklist = &[(30, "Test Land"), (1, "Test Surge")];

/// Storm after twenty spells makes twenty copies in the small layout, whose stack holds 64
/// entries, and each copy's controller is asked about its target without the game stalling
/// ([CR 702.40a]): "copy it for each other spell that was cast before it this turn. If the spell
/// has any targets, you may choose new targets for any of the copies."
///
/// [CR 702.40a]: https://yawgatog.com/resources/magic-rules/#R70240a
#[test]
fn storm_makes_twenty_copies_in_the_small_layout() {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let surge = g.give(g.me, "Test Surge", Zone::Hand);
    // Twenty spells were cast this turn before it.
    g.s.board().set_counted(g.me, Count::SpellsCast, 20);
    g.pick(cast(surge));
    g.pick(Opt::Player(g.me));
    let mut asked = 0;
    let mut deepest = 0;
    while g.s.outcome().is_none() && !(g.s.stack().is_empty() && g.s.triggers().is_empty()) {
        deepest = deepest.max(g.s.stack().len());
        let d = g.current();
        if d.kind == DecisionKind::Target {
            asked += 1;
        }
        // Each copy keeps its target, the first option.
        g.choose(Choice(0));
    }
    assert_eq!(g.s.outcome(), None);
    assert_eq!(asked, 20, "one question for each copy's target");
    assert_eq!(deepest, 21, "the spell and its twenty copies");
    assert_eq!(g.s.player(g.me).life(), 41);
    assert_eq!(
        g.s.counted(Count::SpellsCast, g.me),
        21,
        "copies are not cast"
    );
}
