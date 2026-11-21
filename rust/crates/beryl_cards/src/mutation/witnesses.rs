//! Scripted positions for definition changes the generated scenarios do not yet reach.

use beryl_card::{Castings, Colors, DecisionKind, FaceIx, Subtype, TurnStep, Zone};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use crate::{names::*, props::Prop, tokens::*};

pub(super) fn for_card(card: &'static str) -> Vec<Scenario> {
    match card {
        DRESS_DOWN | FIEND_HUNTER | PORTABLE_HOLE | SNAPCASTER_MAGE | TRINKET_MAGE
        | WORLDGORGER_DRAGON => {
            let mut out = vec![other_permanent_enters(card)];
            if card == WORLDGORGER_DRAGON {
                out.push(EXILE_DRAGON);
            }
            out
        }
        BEZA_THE_BOUNDING_SPRING | CURRENCY_CONVERTER | HULLBREACHER => vec![treasure(card)],
        CAVERN_OF_SOULS => vec![CAVERN],
        AANGS_ICEBERG => vec![WATERBEND],
        SUBTLETY => vec![COUNTERED_EVOKE],
        TUNDRA => vec![no_black_mana(card)],
        BLOODGHAST => vec![HASTE],
        CHAINS_OF_MEPHISTOPHELES => vec![FIRST_DISCARD],
        GET_LOST => vec![tapped_cost(GET_LOST, MAP_TOKEN)],
        LIFE_AND_LIMB => vec![LAND_TYPES, PAINTED_FOREST],
        MINDSLAVER => vec![tapped_cost(MINDSLAVER, MINDSLAVER)],
        OKO_THIEF_OF_CROWNS => vec![tapped_cost(OKO_THIEF_OF_CROWNS, FOOD_TOKEN)],
        SPHINX_OF_FALSE_CONCLUSIONS => vec![EXILE_SPHINX],
        SWIFT_RECONFIGURATION => vec![CREW_FOUR, CREW_FIVE],
        TEFERIS_PROTECTION => vec![UNCHANGING_LIFE],
        TEFERI_TIME_RAVELER => vec![INSTANT_RESTRICTED, INSTANT_PERMISSION],
        TORMODS_CRYPT => vec![tapped_cost(TORMODS_CRYPT, TORMODS_CRYPT)],
        UNDERWORLD_BREACH => vec![OPPONENT_GRAVEYARD],
        _ => Vec::new(),
    }
}

fn other_permanent_enters(card: &'static str) -> Scenario {
    Scenario::new("another permanent entering does not trigger this permanent")
        .battlefield(P0, leak(vec![card, PLAINS, ORNITHOPTER]))
        .hand(P0, &[ISLAND, SUBTLETY])
        .graveyard(P0, &[OUST])
        .battlefield(P1, &[ORNITHOPTER])
        .actions(leak(vec![
            ME.plays(ISLAND),
            advance_to(1, TurnStep::PrecombatMain),
        ]))
}

const COUNTERED_EVOKE: Scenario =
    Scenario::new("the evoked creature stays after Stifle and another permanent entering")
        .battlefield(P0, &[ISLAND, ISLAND])
        .hand(P0, &[SUBTLETY, STIFLE, STIFLE, SNAPCASTER_MAGE, PLAINS])
        .actions(&[
            Action::Cast {
                player: P0,
                card: SUBTLETY,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            ME.picks(SNAPCASTER_MAGE),
            Resolve,
            ME.chooses(DecisionKind::TriggerOrder, SUBTLETY),
            ME.casts(STIFLE),
            ME.targets(SUBTLETY),
            Resolve,
            ME.casts(STIFLE),
            Resolve,
            ME.plays(PLAINS),
            advance_to(1, TurnStep::PrecombatMain),
            ME.controls(SUBTLETY, 1),
        ]);

fn no_black_mana(land: &'static str) -> Scenario {
    Scenario::new("the land cannot pay for a black creature")
        .battlefield(P0, leak(vec![land, PLAINS, PLAINS]))
        .hand(P0, &[Prop::FlashCreature, ISLAND])
        .actions(leak(vec![ME.deciding(DecisionKind::Priority, 2)]))
}

const EXILE_DRAGON: Scenario = Scenario::new("exiling the Dragon returns its linked permanent")
    .battlefield(P0, &[PLAINS, PLAINS])
    .permanents(
        P1,
        &[Permanent::new(WORLDGORGER_DRAGON).exiled_with(&[ISLAND])],
    )
    .hand(P0, &[WINDS_OF_ABANDON])
    .library(P1, &[])
    .actions(&[ME.casts(WINDS_OF_ABANDON)]);

const EXILE_SPHINX: Scenario = Scenario::new("exiling the Sphinx does not make a token")
    .battlefield(P0, &[PLAINS, PLAINS])
    .battlefield(P1, &[SPHINX_OF_FALSE_CONCLUSIONS])
    .hand(P0, &[WINDS_OF_ABANDON])
    .library(P1, &[])
    .actions(&[ME.casts(WINDS_OF_ABANDON)]);

fn tapped_cost(maker: &'static str, source: &'static str) -> Scenario {
    let permanent = if source == maker {
        Permanent::new(source)
    } else {
        Permanent::token(source)
    };
    Scenario::new("a tapped source cannot pay its activation cost")
        .permanents(P0, leak(vec![permanent.tapped()]))
        .battlefield(P0, &[PLAINS, PLAINS, PLAINS, PLAINS, ORNITHOPTER])
        .hand(P0, &[PLAINS])
        .hand(P1, leak(vec![maker]))
        .graveyard(P1, &[COUNTERSPELL])
        .actions(leak(vec![ME.deciding(DecisionKind::Priority, 2)]))
}

fn treasure(maker: &'static str) -> Scenario {
    Scenario::new("a Treasure can pay for a white spell and is sacrificed")
        .permanents(P0, leak(vec![Permanent::token(TREASURE_TOKEN)]))
        .hand(P0, &[PORTABLE_HOLE])
        .hand(P1, leak(vec![maker]))
        .actions(leak(vec![ME.casts(PORTABLE_HOLE)]))
}

const CAVERN: Scenario = Scenario::new("the chosen type can be cast using white mana")
    .hand(P0, &[CAVERN_OF_SOULS, crate::props::FLIER.card])
    .battlefield(P0, &[PLAINS, ISLAND])
    .actions(&[
        ME.plays(CAVERN_OF_SOULS),
        ME.chooses_named("Spirit"),
        ME.casts(crate::props::FLIER.card),
    ]);

const WATERBEND: Scenario = Scenario::new("waterbend can tap all three artifacts to pay three")
    .battlefield(P0, &[AANGS_ICEBERG, ORNITHOPTER, ORNITHOPTER, ORNITHOPTER])
    .actions(&[ME.activates(AANGS_ICEBERG, 1)]);

const HASTE: Scenario = Scenario::new("only Bloodghast gets haste against an opponent at ten")
    .at(2, P1, TurnStep::PrecombatMain)
    .life(P1, 10)
    .battlefield(P0, &[BLOODGHAST, Prop::Vanilla])
    .battlefield(P1, &[Prop::Vanilla])
    .actions(&[
        advance_to(2, TurnStep::BeginningOfCombat),
        THEM.passes(),
        THEM.attacks_with(Prop::Vanilla),
        ME.chooses(DecisionKind::Blocker, Prop::Vanilla),
    ]);

const FIRST_DISCARD: Scenario = Scenario::new("Chains discards one when the player holds two")
    .battlefield(P0, &[CHAINS_OF_MEPHISTOPHELES, ISLAND, ISLAND])
    .hand(P0, &[DRESS_DOWN, PLAINS, ORNITHOPTER])
    .actions(&[
        ME.casts(DRESS_DOWN),
        Resolve,
        Resolve,
        ME.chooses(DecisionKind::ChooseObject, PLAINS),
        ME.has(Zone::Graveyard, ORNITHOPTER, 0),
    ]);

const LAND_TYPES: Scenario = Scenario::new("a changeling keeps artifact and gains Forest")
    .battlefield(
        P0,
        &[LIFE_AND_LIMB, UNIVERSAL_AUTOMATON, SPARAS_HEADQUARTERS],
    )
    .hand(P0, &[ISLAND])
    .actions(&[
        ME.has_subtype(UNIVERSAL_AUTOMATON, Subtype::Forest, true),
        ME.has_subtype(SPARAS_HEADQUARTERS, Subtype::Island, true),
    ]);

const PAINTED_FOREST: Scenario = Scenario::new("Life and Limb replaces a Forest's earlier color")
    .battlefield(P0, &[PLAINS, PLAINS, PLAINS, PLAINS, PLAINS, FOREST])
    .hand(P0, &[PAINTERS_SERVANT, LIFE_AND_LIMB, ISLAND])
    .actions(&[
        ME.casts(PAINTERS_SERVANT),
        Resolve,
        ME.chooses_color(Colors::RED),
        ME.casts(LIFE_AND_LIMB),
        Resolve,
        ME.colored(Zone::Battlefield, FOREST, Colors::GREEN),
    ]);

const CREW_FOUR: Scenario = Scenario::new("four power cannot crew the Vehicle")
    .hand(P0, &[SWIFT_RECONFIGURATION, ISLAND])
    .battlefield(P0, &[PLAINS, ORNITHOPTER, Prop::Vanilla, Prop::Vanilla])
    .actions(&[
        ME.casts(SWIFT_RECONFIGURATION),
        ME.targets(ORNITHOPTER),
        Resolve,
        ME.deciding(DecisionKind::Priority, 2),
    ]);

const CREW_FIVE: Scenario = Scenario::new("five power can crew the Vehicle")
    .hand(P0, &[SWIFT_RECONFIGURATION])
    .battlefield(P0, &[PLAINS, ORNITHOPTER, Prop::Vanilla, Prop::Flier])
    .actions(&[
        ME.casts(SWIFT_RECONFIGURATION),
        ME.targets(ORNITHOPTER),
        Resolve,
        Action::activate_granted(P0, ORNITHOPTER, 0),
    ]);

const UNCHANGING_LIFE: Scenario = Scenario::new("protection also stops a gain of life")
    .life(P0, 10)
    .battlefield(P0, &[PLAINS; 7])
    .hand(P0, &[TEFERIS_PROTECTION, BEZA_THE_BOUNDING_SPRING])
    .actions(&[
        ME.casts(BEZA_THE_BOUNDING_SPRING),
        Resolve,
        ME.casts(TEFERIS_PROTECTION),
        Resolve,
        Resolve,
        ME.life(10),
    ]);

const INSTANT_RESTRICTED: Scenario =
    Scenario::new("Teferi restricts the opponent's instants as well")
        .at(2, P1, TurnStep::Upkeep)
        .battlefield(P0, &[TEFERI_TIME_RAVELER, ORNITHOPTER])
        .battlefield(P1, &[PLAINS])
        .hand(P1, &[DEATH_WARD])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            THEM.deciding(DecisionKind::Priority, 1),
        ]);

const INSTANT_PERMISSION: Scenario =
    Scenario::new("Teferi's permission applies to sorceries belonging to his controller")
        .battlefield(
            P0,
            &[TEFERI_TIME_RAVELER, PLAINS, PLAINS, PLAINS, ORNITHOPTER],
        )
        .battlefield(P1, &[PLAINS, ORNITHOPTER])
        .hand(P0, &[OUST, PORTABLE_HOLE, GET_LOST])
        .hand(P1, &[OUST])
        .actions(&[
            ME.activates(TEFERI_TIME_RAVELER, 1),
            Resolve,
            ME.casts(GET_LOST),
            ME.targets(TEFERI_TIME_RAVELER),
            Resolve,
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.deciding(DecisionKind::Priority, 2),
            ME.passes(),
        ]);

const OPPONENT_GRAVEYARD: Scenario = Scenario::new("the opponent's graveyard cards cannot escape")
    .at(2, P1, TurnStep::PrecombatMain)
    .battlefield(P0, &[UNDERWORLD_BREACH])
    .battlefield(P1, &[PLAINS])
    .graveyard(P1, &[PORTABLE_HOLE, ISLAND, ISLAND, ISLAND])
    .hand(P1, &[PLAINS])
    .actions(&[THEM.deciding(DecisionKind::Priority, 2)]);

fn leak<T>(values: Vec<T>) -> &'static [T] {
    Vec::leak(values)
}
