use beryl_card::definition::Shape;
use beryl_card::{
    Ability, CardDef, Castings, Face, FaceIx, ManaColor, ManaCost, Types, card, card_with_shape,
};

use super::LAND;
use super::cast::{Game, cast, index_of};
use crate::game;
use crate::state::Zone;
use crate::{CardTable, DecisionKind, Decklist, Observation, Opt, Profile};

const fn spell(cost: u8) -> Face {
    Face::EMPTY
        .cost(ManaCost::generic(cost))
        .types(Types::SORCERY)
}

static CASCADER: CardDef = card("Test Cascade", &[spell(4).abilities(&[Ability::cascade()])]);
static VARIABLE: CardDef = card(
    "Test Variable Cascade",
    &[spell(0)
        .cost(ManaCost {
            x: 1,
            ..ManaCost::generic(2)
        })
        .abilities(&[Ability::cascade()])],
);
const SPLIT: Shape = Shape::Split { fuse: false };
const MODAL_SHAPE: Shape = Shape::Modal;
static EXPENSIVE_SPLIT: CardDef = card_with_shape(
    "Test Expensive Split",
    SPLIT,
    &[
        spell(2).for_shape(SPLIT, FaceIx::FRONT),
        spell(2).for_shape(SPLIT, FaceIx::BACK),
    ],
);
static CHEAP_SPLIT: CardDef = card_with_shape(
    "Test Cheap Split",
    SPLIT,
    &[
        spell(1).for_shape(SPLIT, FaceIx::FRONT),
        spell(2).for_shape(SPLIT, FaceIx::BACK),
    ],
);
static MODAL: CardDef = card_with_shape(
    "Test Modal",
    MODAL_SHAPE,
    &[
        spell(1).for_shape(MODAL_SHAPE, FaceIx::FRONT),
        spell(4).for_shape(MODAL_SHAPE, FaceIx::BACK),
    ],
);
static HIT: CardDef = card("Test Hit", &[spell(3)]);

static CARDS: CardTable = CardTable::new(&[
    &LAND,
    &CASCADER,
    &VARIABLE,
    &EXPENSIVE_SPLIT,
    &CHEAP_SPLIT,
    &MODAL,
    &HIT,
]);

const DECK: &Decklist = &[
    (16, LAND.name),
    (1, CASCADER.name),
    (1, VARIABLE.name),
    (1, EXPENSIVE_SPLIT.name),
    (1, CHEAP_SPLIT.name),
    (1, MODAL.name),
    (2, HIT.name),
];

fn prepare(card: &CardDef) -> (Game, crate::ids::Slot) {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    let source = g.give(g.me, card.name, Zone::Hand);
    let added = g.s.add_mana(g.me, None, ManaColor::White, 4);
    g.x.events.push(added);
    (g, source)
}

#[test]
fn cascade_uses_combined_split_cost_and_offers_each_half() {
    let (mut g, source) = prepare(&CASCADER);
    let eligible = g.on_top(g.me, CHEAP_SPLIT.name);
    let skipped = g.on_top(g.me, EXPENSIVE_SPLIT.name);

    g.pick(cast(source));
    g.pass_both();
    let offer = g.current();
    let back = Opt::Cast {
        slot: eligible,
        face: FaceIx::BACK,
        costs: Castings::NONE,
    };
    assert_eq!(offer.kind, DecisionKind::CastOffer);
    assert_eq!(offer.len(), 3);
    assert!(index_of(&offer, cast(eligible)).is_some());
    assert!(index_of(&offer, back).is_some());
    g.pick(back);
    assert_eq!(g.s.stack()[1].face(), FaceIx::BACK);
    assert_eq!(g.s.mana_value(&g.t, eligible), 2);
    assert_eq!(g.s.object(skipped).zone(), Zone::Library);
}

#[test]
fn cascade_checks_the_resulting_modal_face() {
    let (mut g, source) = prepare(&CASCADER);
    let modal = g.on_top(g.me, MODAL.name);

    g.pick(cast(source));
    g.pass_both();
    let offer = g.current();
    assert_eq!(offer.kind, DecisionKind::CastOffer);
    assert_eq!(offer.len(), 2);
    assert!(index_of(&offer, cast(modal)).is_some());
    g.pick(cast(modal));
    assert_eq!(g.s.stack()[1].face(), FaceIx::FRONT);
}

#[test]
fn cascade_uses_the_chosen_x_of_a_countered_spell() {
    let (mut g, source) = prepare(&VARIABLE);
    let hit = g.on_top(g.me, HIT.name);
    g.pick(cast(source));
    g.pick(Opt::Number(2));
    assert_eq!(g.s.mana_value(&g.t, source), 4);
    let spell = g.s.object_ref(source);
    let mut game = game::Game::new(&mut g.s, &g.t, &mut g.x.events);
    assert!(crate::actions::counter::counter(&mut game, spell, Zone::Graveyard).unwrap());

    g.pass_both();
    assert_eq!(g.current().kind, DecisionKind::CastOffer);
    g.pick(cast(hit));
    assert_eq!(g.s.object(source).zone(), Zone::Graveyard);
    assert_eq!(g.s.object(hit).zone(), Zone::Stack);
}

#[test]
fn a_copied_cascade_trigger_exiles_its_own_cards() {
    let (mut g, source) = prepare(&CASCADER);
    let second = g.on_top(g.me, MODAL.name);
    let first = g.on_top(g.me, HIT.name);
    let land = g.on_top(g.me, LAND.name);
    g.pick(cast(source));

    let trigger = g.s.stack()[1];
    g.s.put_on_stack(&mut g.x.events, trigger).unwrap();
    g.pass_both();
    assert_eq!(g.current().kind, DecisionKind::CastOffer);
    let shown = crate::view(&g.s, &g.t, &mut g.x);
    let opponent = Observation::paused(shown, &g.s, g.them);
    for slot in [land, first] {
        assert_eq!(shown.object(slot).zone(), Zone::Exile);
        assert!(opponent.knows(slot));
    }
    g.pick(cast(first));
    assert_eq!(g.s.object(land).zone(), Zone::Library);
    g.pass_both();
    g.pass_both();
    assert_eq!(g.current().kind, DecisionKind::CastOffer);
    g.pick(cast(second));
    assert_eq!(g.s.object(first).zone(), Zone::Graveyard);
    assert_eq!(g.s.object(second).zone(), Zone::Stack);
    assert_eq!(g.s.stack().len(), 2);
}
