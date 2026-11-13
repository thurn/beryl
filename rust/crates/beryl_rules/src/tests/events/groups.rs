//! Which recorded events happened as one event of the rules: the groups a recording sink's
//! tags tell, and the blocks a declaration says.

use beryl_card::effect::build::{destroy, draw, exile_until_this_leaves, put_back, target, you};
use beryl_card::{Amount, Effect, Objects, Recipients};

use crate::card::{
    Ability, CardDef, CostPart, Face, ManaCost, Players, Subtype, Subtypes, TargetKind, TargetSpec,
    Trigger, Types,
};
use crate::event::{Event, Reason, Recipient, Tag};
use crate::filter::Filter;
use crate::state::Zone;
use crate::tests::LAND;
use crate::tests::cast::{Game, cast, index_of};
use crate::{CardTable, Choice, DecisionKind, Decklist, Opt, Profile, Scratch};

const W: ManaCost = ManaCost::new(0, [1, 0, 0, 0, 0, 0]);
const WW: ManaCost = ManaCost::new(0, [2, 0, 0, 0, 0, 0]);
const CREATURE: Filter = Filter::Types(Types::CREATURE);
const EACH_CREATURE: Objects = Objects::All {
    zone: Zone::Battlefield,
    filter: CREATURE,
};

static BEAR: CardDef = CardDef {
    name: "Group Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static OGRE: CardDef = CardDef {
    name: "Group Ogre",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(3),
        toughness: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static AURA: CardDef = CardDef {
    name: "Group Aura",
    faces: &[Face {
        types: Types::ENCHANTMENT,
        subtypes: Subtypes::of(&[Subtype::Aura]),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Destroy all creatures."
static WRATH: CardDef = CardDef {
    name: "Group Wrath",
    faces: &[Face {
        cost: WW,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&destroy(EACH_CREATURE))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "This deals 2 damage to each creature."
static QUAKE: CardDef = CardDef {
    name: "Group Quake",
    faces: &[Face {
        cost: W,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&Effect::DealDamage {
            to: Recipients::Permanents(EACH_CREATURE),
            amount: Amount::Fixed(2),
        })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Draw two cards."
static INSIGHT: CardDef = CardDef {
    name: "Group Insight",
    faces: &[Face {
        cost: W,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&draw(you(), 2))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "When this enters, exile target creature an opponent controls until this leaves the
/// battlefield."
static JAILER: CardDef = CardDef {
    name: "Group Jailer",
    faces: &[Face {
        cost: W,
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::ENTERS)
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::All(&[CREATURE, Filter::Whose(&Players::EachOpponent)]),
            )])
            .effect(&exile_until_this_leaves(target()))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "As an additional cost to cast this spell, exile two cards from your graveyard. Draw a
/// card."
static DELVER: CardDef = CardDef {
    name: "Group Delver",
    faces: &[Face {
        cost: W,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&draw(you(), 1))],
        ..Face::EMPTY
    }
    .additional_cost(&[CostPart::exile(2, Zone::Graveyard, Filter::Any)])],
    tokens: &[],
};

/// "Put two cards from your hand on top of your library in any order."
static STORM: CardDef = CardDef {
    name: "Group Storm",
    faces: &[Face {
        cost: W,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&put_back(you(), 2))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND, &BEAR, &OGRE, &AURA, &WRATH, &QUAKE, &INSIGHT, &JAILER, &DELVER, &STORM,
]);

const DECK: &Decklist = &[
    (20, "Test Land"),
    (4, "Group Bear"),
    (2, "Group Ogre"),
    (2, "Group Aura"),
    (2, "Group Wrath"),
    (2, "Group Quake"),
    (4, "Group Insight"),
    (2, "Group Jailer"),
    (1, "Group Delver"),
    (1, "Group Storm"),
];

/// A game of [`DECK`] mirrors in the starting player's main phase, recording its events.
fn game() -> Game {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    g.x = Scratch::recording();
    g
}

/// The events recorded since the last call, each with its group.
fn told(g: &mut Game) -> Vec<(Event, u32)> {
    assert!(g.x.events.is_idle(), "a scope or a group left open");
    let groups = Tag::groups(g.x.events.tags());
    let events = g.x.events.drain().collect::<Vec<_>>();
    events.into_iter().zip(groups).collect()
}

/// The one group the events that `is` share with each other and with no other event but
/// those that `also` is.
fn group_with(
    events: &[(Event, u32)],
    is: impl Fn(&Event) -> bool,
    also: impl Fn(&Event) -> bool,
) -> u32 {
    let mut found = events
        .iter()
        .filter(|(e, _)| is(e))
        .map(|&(_, group)| group);
    let group = found
        .next()
        .unwrap_or_else(|| panic!("no such event in {events:#?}"));
    assert!(found.all(|other| other == group), "apart: {events:#?}");
    let others = events
        .iter()
        .filter(|(e, of)| *of == group && !is(e) && !also(e));
    assert_eq!(others.count(), 0, "others with them: {events:#?}");
    group
}

/// The one group of exactly the events that `is`.
fn group(events: &[(Event, u32)], is: impl Fn(&Event) -> bool) -> u32 {
    group_with(events, is, |_| false)
}

fn count(events: &[(Event, u32)], is: impl Fn(&Event) -> bool) -> usize {
    events.iter().filter(|(e, _)| is(e)).count()
}

fn moves(reason: Reason) -> impl Fn(&Event) -> bool {
    move |e| matches!(*e, Event::ZoneChange { reason: r, .. } if r == reason)
}

/// Whether the event is said for whoever follows the stack.
fn of_the_stack(e: &Event) -> bool {
    matches!(e, Event::StackArrival { .. } | Event::StackDeparture { .. })
}

/// Both players pass in succession, and the game goes on to the next decision.
fn pass(g: &mut Game) {
    g.pass_both();
    g.decide();
}

#[test]
fn a_sweepers_permanents_leave_as_one_event_whoever_owns_them() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    for _ in 0..2 {
        g.give(me, "Test Land", Zone::Battlefield);
    }
    g.give(me, "Group Bear", Zone::Battlefield);
    g.give(them, "Group Bear", Zone::Battlefield);
    g.give(them, "Group Ogre", Zone::Battlefield);
    let wrath = g.give(me, "Group Wrath", Zone::Hand);
    g.pick(cast(wrath));
    told(&mut g);
    pass(&mut g);
    let events = told(&mut g);
    // CR 608.2f: "Some spells and abilities include actions taken on multiple players and/or
    // objects. In most cases, each such action is processed simultaneously."
    assert_eq!(count(&events, moves(Reason::Destroy)), 3);
    let destroyed = group(&events, moves(Reason::Destroy));
    // CR 608.2n: the spell is put into its owner's graveyard "as the final step" of its
    // resolution, after its instructions are followed.
    assert_ne!(group(&events, moves(Reason::Resolve)), destroyed);
}

#[test]
fn a_payments_taps_and_mana_are_one_event_and_the_rest_of_the_cast_is_not() {
    let mut g = game();
    let me = g.me;
    for _ in 0..2 {
        g.give(me, "Test Land", Zone::Battlefield);
    }
    let wrath = g.give(me, "Group Wrath", Zone::Hand);
    g.pick(cast(wrath));
    let events = told(&mut g);
    // CR 601.2h: "The player pays the total cost. ... Partial payments are not allowed."
    let paying = |e: &Event| matches!(e, Event::Tap { .. } | Event::Mana { .. });
    assert_eq!(count(&events, paying), 4);
    let paid = group(&events, paying);
    // CR 601.2a: the card moved to the stack before any cost was known, and CR 601.2i: "Once
    // the steps described in 601.2a-h are completed, ... the spell becomes cast."
    assert!(group_with(&events, moves(Reason::Cast), of_the_stack) < paid);
    assert!(group(&events, |e| matches!(e, Event::Cast { .. })) > paid);
}

#[test]
fn cards_drawn_by_one_instruction_are_drawn_one_at_a_time() {
    let mut g = game();
    let me = g.me;
    g.give(me, "Test Land", Zone::Battlefield);
    let insight = g.give(me, "Group Insight", Zone::Hand);
    g.pick(cast(insight));
    told(&mut g);
    pass(&mut g);
    let events = told(&mut g);
    // CR 121.2: "Cards may only be drawn one at a time. If a player is instructed to draw
    // multiple cards, that player performs that many individual card draws."
    let drawn: Vec<u32> = events
        .iter()
        .filter(|(e, _)| matches!(e, Event::DrawCard { .. }))
        .map(|&(_, group)| group)
        .collect();
    assert_eq!(drawn.len(), 2);
    assert_ne!(drawn[0], drawn[1]);
}

#[test]
fn deaths_of_one_state_based_check_are_one_event_and_the_next_checks_another() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    g.give(me, "Test Land", Zone::Battlefield);
    let mine = g.give(me, "Group Bear", Zone::Battlefield);
    let theirs = g.give(them, "Group Bear", Zone::Battlefield);
    let aura = g.give(me, "Group Aura", Zone::Battlefield);
    g.s.board().attach(aura, mine);
    let quake = g.give(me, "Group Quake", Zone::Hand);
    g.pick(cast(quake));
    told(&mut g);
    pass(&mut g);
    let events = told(&mut g);
    // CR 608.2f: the damage to each creature is one action.
    assert_eq!(count(&events, |e| matches!(e, Event::Damage { .. })), 2);
    let damaged = group(&events, |e| matches!(e, Event::Damage { .. }));
    // CR 704.3: "the game checks for any of the listed conditions for state-based actions,
    // then performs all applicable state-based actions simultaneously as a single event. If
    // any state-based actions are performed as a result of a check, the check is repeated".
    // Both creatures were dealt lethal damage (CR 704.5g) as the first check found them.
    let died = |slot| move |e: &Event| matches!(*e, Event::ZoneChange { object, reason: Reason::StateBased, .. } if object.slot == slot);
    let first = group(&events, |e| died(mine)(e) || died(theirs)(e));
    assert!(damaged < first);
    // The Aura was attached until then: only the repeated check finds it attached to nothing
    // (CR 704.5m).
    assert!(group(&events, died(aura)) > first);
}

#[test]
fn what_a_permanent_exiled_returns_with_its_leaving() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    for _ in 0..2 {
        g.give(me, "Test Land", Zone::Battlefield);
    }
    let bear = g.give(them, "Group Bear", Zone::Battlefield);
    let jailer = g.give(me, "Group Jailer", Zone::Hand);
    let quake = g.give(me, "Group Quake", Zone::Hand);
    g.pick(cast(jailer));
    // The enters trigger has its one target, and resolves in turn with nothing to answer it.
    pass(&mut g);
    assert_eq!(g.s.object(bear).zone(), Zone::Exile);
    g.pick(cast(quake));
    told(&mut g);
    pass(&mut g);
    let events = told(&mut g);
    // CR 610.3: "A second one-shot effect is created immediately after the specified event.
    // This second one-shot effect returns the object to its previous zone." Nothing is
    // between the two, and they are told as one group, the return by its reason.
    let died = |e: &Event| matches!(*e, Event::ZoneChange { object, reason: Reason::StateBased, .. } if object.slot == jailer);
    assert_eq!(count(&events, died), 1);
    assert_eq!(count(&events, moves(Reason::Return)), 1);
    group(&events, |e| died(e) || moves(Reason::Return)(e));
    assert_eq!(g.s.object(bear).zone(), Zone::Battlefield);
}

#[test]
fn a_declaration_is_one_event_and_so_is_the_combat_damage() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    let bear = g.give(me, "Group Bear", Zone::Battlefield);
    let ogre = g.give(me, "Group Ogre", Zone::Battlefield);
    let blocker = g.give(them, "Group Bear", Zone::Battlefield);
    let declared = Event::Block {
        player: them,
        blocker: g.s.object_ref(blocker),
        attacker: g.s.object_ref(bear),
    };
    // Both attack, the defender's Bear blocks the attacking Bear, and every other decision
    // takes its first option: a pass, or "done". The Bears trade.
    while g.s.object(blocker).zone() == Zone::Battlefield {
        let d = g.decide();
        if d.kind == DecisionKind::BlockTarget {
            g.pick_object(bear);
            continue;
        }
        let wanted = [Opt::Attack(bear), Opt::Attack(ogre), Opt::Block(blocker)];
        let wanted = wanted.into_iter().find_map(|opt| index_of(&d, opt));
        g.choose(wanted.unwrap_or(Choice(0)));
    }
    let events = told(&mut g);
    // CR 508.1f: "The active player taps the chosen creatures." The taps are of the one
    // declaration (CR 508.1).
    let attacking = |e: &Event| matches!(e, Event::Tap { .. } | Event::Attack { .. });
    assert_eq!(count(&events, attacking), 3);
    let attacked = group(&events, attacking);
    // CR 509.1a: "For each of the chosen creatures, the defending player chooses one
    // creature for it to block".
    let blocks: Vec<&Event> = events
        .iter()
        .map(|(e, _)| e)
        .filter(|e| matches!(e, Event::Block { .. }))
        .collect();
    assert_eq!(blocks, [&declared]);
    let declaring = group(&events, |e| matches!(e, Event::Block { .. }));
    // CR 510.2: "all combat damage that's been assigned is dealt simultaneously." The damage
    // to the player is the life they lose (CR 120.3a: "Damage dealt to a player by a source
    // without infect causes that player to lose that much life."), which no other event says.
    let blow = |e: &Event| matches!(e, Event::Damage { .. } | Event::LifeLoss { .. });
    assert_eq!(count(&events, blow), 3);
    assert_eq!(
        count(&events, |e| matches!(
            *e,
            Event::Damage { to: Recipient::Player(player), amount: 3, .. } if player == them
        )),
        1
    );
    let dealt = group(&events, blow);
    // CR 704.3: the two Bears, of different owners, die in the one check that follows.
    assert_eq!(count(&events, moves(Reason::StateBased)), 2);
    let died = group(&events, moves(Reason::StateBased));
    assert!(attacked < declaring && declaring < dealt && dealt < died);
    assert_eq!(g.s.player(them).life(), 17);
}

#[test]
fn the_cards_a_cost_exiles_leave_as_one_event_after_the_last_is_chosen() {
    let mut g = game();
    let me = g.me;
    g.give(me, "Test Land", Zone::Battlefield);
    // Three cards no two alike, so each choice is asked.
    let cards = ["Group Bear", "Group Ogre", "Group Aura"].map(|c| g.give(me, c, Zone::Graveyard));
    let delver = g.give(me, "Group Delver", Zone::Hand);
    g.pick(cast(delver));
    // Each card is chosen by its own answer, and none leaves until the last is chosen.
    let mut picks = 0;
    while g.current().kind == DecisionKind::ChooseObject {
        assert!(
            cards
                .iter()
                .all(|&c| g.s.object(c).zone() == Zone::Graveyard)
        );
        g.choose(Choice(0));
        picks += 1;
    }
    assert_eq!(picks, 2);
    let events = told(&mut g);
    // CR 601.2h: "The player pays the total cost." The two cards are one payment of one
    // cost, exiled at once, after the mana.
    assert_eq!(count(&events, moves(Reason::Exile)), 2);
    let exiled = group(&events, moves(Reason::Exile));
    let paid = group(&events, |e| {
        matches!(e, Event::Tap { .. } | Event::Mana { .. })
    });
    assert!(paid < exiled);
    let left = cards
        .iter()
        .filter(|&&c| g.s.object(c).zone() == Zone::Graveyard);
    assert_eq!(left.count(), 1);
}

#[test]
fn cards_put_back_from_a_hand_go_to_the_library_as_one_event_in_the_order_chosen() {
    let mut g = game();
    let me = g.me;
    g.give(me, "Test Land", Zone::Battlefield);
    let first = g.give(me, "Group Bear", Zone::Hand);
    let second = g.give(me, "Group Ogre", Zone::Hand);
    let storm = g.give(me, "Group Storm", Zone::Hand);
    g.pick(cast(storm));
    told(&mut g);
    g.pass_both();
    // The second card is the one left to choose, and is chosen unasked.
    g.pick_object(first);
    let events = told(&mut g);
    // CR 401.4: "If an effect puts two or more cards in a specific position in a library at
    // the same time, the owner of those cards may arrange them in any order." One event puts
    // both there, each chosen going on top of the one before.
    let put = |e: &Event| {
        matches!(
            *e,
            Event::ZoneChange {
                to: Zone::Library,
                ..
            }
        )
    };
    assert_eq!(count(&events, put), 2);
    group(&events, put);
    let library = g.s.library(me);
    assert_eq!(library[library.len() - 2..], [first, second]);
}
