//! What each recorded event comes from, and which spell or ability on the stack that is: the
//! tags a recording sink keeps beside its events, read as someone who follows the stack
//! through the events alone reads them.

use beryl_card::effect::build::{
    counter, create_tokens, destroy, discard, draw, its_controller, sequence, target, unless_pays,
    you,
};
use beryl_card::{Amount, Effect, Recipients};

use crate::card::{
    Ability, CardDef, Cost, CostPart, Face, ManaCost, Players, TargetKind, TargetSpec, Trigger,
    Types,
};
use crate::event::{Cause, Event, Reason, StackIx, Tagged};
use crate::filter::Filter;
use crate::ids::{Seat, Slot};
use crate::state::{StackEntry, TurnStep, Zone};
use crate::tests::LAND;
use crate::tests::cast::{Game, cast, index_of};
use crate::{CardTable, Choice, DecisionKind, Decklist, Opt, Profile, Scratch, paused_events};

const W: ManaCost = ManaCost::new(0, [1, 0, 0, 0, 0, 0]);
const CREATURE: Filter = Filter::Types(Types::CREATURE);

static INSECT: CardDef = CardDef {
    name: "Cause Insect",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Draw a card."
static SAGE: CardDef = CardDef {
    name: "Cause Sage",
    faces: &[Face {
        cost: W,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&draw(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Draw a card, then discard a card.": it asks which, with the draw done.
static LOOT: CardDef = CardDef {
    name: "Cause Loot",
    faces: &[Face {
        cost: W,
        types: Types::SORCERY,
        abilities: &[Ability::spell().effect(&sequence(&[draw(you(), 1), discard(you(), 1)]))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "This deals 2 damage to target creature."
static SHOCK: CardDef = CardDef {
    name: "Cause Shock",
    faces: &[Face {
        cost: W,
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Permanent, CREATURE)])
            .effect(&Effect::DealDamage {
                to: Recipients::Permanents(target()),
                amount: Amount::Fixed(2),
            })],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Counter target spell."
static DENY: CardDef = CardDef {
    name: "Cause Deny",
    faces: &[Face {
        cost: W,
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
            .effect(&counter(target()))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Counter target spell unless its controller pays {1}."
static TITHE: CardDef = CardDef {
    name: "Cause Tithe",
    faces: &[Face {
        cost: W,
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Spell, Filter::Any)])
            .effect(&unless_pays(
                &counter(target()),
                its_controller(),
                ManaCost::generic(1),
            ))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static BEAR: CardDef = CardDef {
    name: "Cause Bear",
    faces: &[Face {
        cost: W,
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "When this enters, create a 1/1 Insect creature token."
static HATCHER: CardDef = CardDef {
    name: "Cause Hatcher",
    faces: &[Face {
        cost: W,
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::ENTERS).effect(&create_tokens(
            you(),
            &INSECT,
            1,
        ))],
        ..Face::EMPTY
    }],
    tokens: &[&INSECT],
};

/// "When this enters, destroy target creature an opponent controls."
static STINGER: CardDef = CardDef {
    name: "Cause Stinger",
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
            .effect(&destroy(target()))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "{T}, Sacrifice this land: Destroy target land."
static STRIP: CardDef = CardDef {
    name: "Cause Strip",
    faces: &[Face {
        types: Types::LAND,
        abilities: &[
            Ability::activated(Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE]))
                .targeting(&[TargetSpec::one(
                    TargetKind::Permanent,
                    Filter::Types(Types::LAND),
                )])
                .effect(&destroy(target())),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "When this dies, draw a card."
static MOURNER: CardDef = CardDef {
    name: "Cause Mourner",
    faces: &[Face {
        cost: W,
        types: Types::CREATURE,
        power: Some(1),
        toughness: Some(1),
        abilities: &[Ability::triggered(&Trigger::DIES).effect(&draw(you(), 1))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// "Destroy target creature."
static DOOM: CardDef = CardDef {
    name: "Cause Doom",
    faces: &[Face {
        cost: W,
        types: Types::INSTANT,
        abilities: &[Ability::spell()
            .targeting(&[TargetSpec::one(TargetKind::Permanent, CREATURE)])
            .effect(&destroy(target()))],
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[
    &LAND, &SAGE, &LOOT, &SHOCK, &DENY, &TITHE, &BEAR, &HATCHER, &STINGER, &STRIP, &MOURNER, &DOOM,
]);

const DECK: &Decklist = &[
    (20, "Test Land"),
    (3, "Cause Sage"),
    (2, "Cause Loot"),
    (2, "Cause Shock"),
    (2, "Cause Deny"),
    (2, "Cause Tithe"),
    (3, "Cause Bear"),
    (2, "Cause Hatcher"),
    (2, "Cause Stinger"),
    (2, "Cause Strip"),
    (1, "Cause Mourner"),
    (1, "Cause Doom"),
];

/// A game of [`DECK`] mirrors in the starting player's main phase, recording its events.
fn game() -> Game {
    let mut g = Game::with(&CARDS, DECK, Profile::EXACT);
    g.x = Scratch::recording();
    g
}

/// What an event came from, with the spell or ability a cause names told by its card's or its
/// source's slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Who {
    Rules,
    Player(Seat, Option<Slot>),
    Effect(Slot),
    Object(Slot, Option<Slot>),
}

/// The events recorded since the last call, each with what it came from.
///
/// It follows the stack as a reader of the events does: an arrival goes on top, a departure
/// takes the entry at its place out, and a cause's place is looked up in what is there then.
/// It fails if a cause names a place nothing holds, but for the one event the rules put
/// ahead of an arrival
/// ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "a player first moves
/// that card ... from where it is to the stack. It becomes the topmost object on the
/// stack."), which names the place its spell is about to take.
fn told(g: &mut Game, stack: &mut Vec<StackEntry>) -> Vec<(Event, Who)> {
    assert!(g.x.events.is_idle(), "a scope left open");
    assert_eq!(g.x.events.events().len(), g.x.events.tags().len());
    read(&g.x.events.drain_tagged().collect::<Vec<_>>(), stack)
}

fn read(events: &[Tagged], stack: &mut Vec<StackEntry>) -> Vec<(Event, Who)> {
    let source = |entry: &StackEntry| entry.object.expect("a source").slot;
    let mut out = Vec::new();
    for (i, e) in events.iter().enumerate() {
        if let Event::StackArrival { entry } = e.event {
            stack.push(entry);
        }
        let named = |place: StackIx| {
            if let Some(entry) = stack.get(place.index()) {
                return source(entry);
            }
            let Event::ZoneChange {
                object,
                to: Zone::Stack,
                ..
            } = e.event
            else {
                panic!("{e:?} names a place nothing holds: {stack:?}");
            };
            assert_eq!(place.index(), stack.len(), "the place above the top");
            let next = events[i + 1..].iter().find_map(|later| match later.event {
                Event::StackArrival { entry } => Some(source(&entry)),
                _ => None,
            });
            assert_eq!(next, Some(object.slot), "the spell arrives next");
            object.slot
        };
        let who = match e.tag.cause {
            Cause::Rules => Who::Rules,
            Cause::Player { player, entry } => Who::Player(player, entry.map(named)),
            Cause::Effect { entry } => Who::Effect(named(entry)),
            Cause::Object { source, entry } => Who::Object(source.slot, entry.map(named)),
        };
        if let Event::StackDeparture { entry } = e.event {
            stack.remove(entry.index());
        }
        out.push((e.event, who));
    }
    out
}

/// What the events that `is` came from: at least one, and all from the same.
fn who(events: &[(Event, Who)], is: impl Fn(&Event) -> bool) -> Who {
    let mut found = events.iter().filter(|(e, _)| is(e)).map(|&(_, who)| who);
    let first = found
        .next()
        .unwrap_or_else(|| panic!("no such event in {events:?}"));
    assert!(found.all(|other| other == first), "{events:?}");
    first
}

fn moves(reason: Reason) -> impl Fn(&Event) -> bool {
    move |e| matches!(*e, Event::ZoneChange { reason: r, .. } if r == reason)
}

fn arrival(e: &Event) -> bool {
    matches!(e, Event::StackArrival { .. })
}

fn departure(e: &Event) -> bool {
    matches!(e, Event::StackDeparture { .. })
}

/// Passes until both players have passed in succession and the top of the stack has resolved
/// or the step has ended, then goes on to the next decision.
fn pass(g: &mut Game) {
    g.pass_both();
    if g.s.outcome().is_none() {
        g.decide();
    }
}

#[test]
fn a_spell_cast_with_mana_is_its_casters_doing_and_its_mana_a_lands_effect() {
    let mut g = game();
    let me = g.me;
    let land = g.give(me, "Test Land", Zone::Battlefield);
    let played = g.give(me, "Test Land", Zone::Hand);
    let sage = g.give(me, "Cause Sage", Zone::Hand);
    let mut stack = Vec::new();

    // CR 116.2a: "Playing a land is a special action."
    g.pick(Opt::PlayLand(played));
    let events = told(&mut g, &mut stack);
    assert_eq!(who(&events, moves(Reason::Play)), Who::Player(me, None));

    // CR 601.2a, 601.2h: every step of casting is the player's, and names the spell.
    g.pick(cast(sage));
    let events = told(&mut g, &mut stack);
    let casting = Who::Player(me, Some(sage));
    assert_eq!(who(&events, moves(Reason::Cast)), casting);
    assert_eq!(who(&events, arrival), casting);
    assert_eq!(who(&events, |e| matches!(e, Event::Tap { .. })), casting);
    assert_eq!(who(&events, |e| matches!(e, Event::Cast { .. })), casting);
    // CR 605.3b: the mana ability "resolves immediately after it is activated": the mana is
    // its effect, and pays for the spell (CR 605.3a).
    let Event::Tap { object: tapped } = events
        .iter()
        .find(|(e, _)| matches!(e, Event::Tap { .. }))
        .unwrap()
        .0
    else {
        unreachable!()
    };
    assert!([land, played].contains(&tapped.slot));
    assert_eq!(
        who(&events, |e| matches!(e, Event::Mana { .. })),
        Who::Object(tapped.slot, Some(sage))
    );
    assert_eq!(stack.len(), 1);

    // CR 608.2c, 608.2n: what it does, and its going to the graveyard, are its effect.
    pass(&mut g);
    let events = told(&mut g, &mut stack);
    let effect = Who::Effect(sage);
    assert_eq!(who(&events, |e| matches!(e, Event::Draw { .. })), effect);
    assert_eq!(who(&events, moves(Reason::Resolve)), effect);
    assert_eq!(who(&events, departure), effect);
    assert!(stack.is_empty());
}

#[test]
fn an_abilitys_sacrifice_is_its_activators_doing_and_names_the_ability() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    let strip = g.give(me, "Cause Strip", Zone::Battlefield);
    let theirs = g.give(them, "Test Land", Zone::Battlefield);
    let mut stack = Vec::new();

    let d = g.decide();
    let activate = (0..d.len() as u16)
        .map(Choice)
        .find(|&c| matches!(d.candidates.option(c), Some(Opt::Activate { slot, .. }) if slot == strip))
        .expect("the ability offered");
    g.choose(activate);
    g.pick_object(theirs);
    let events = told(&mut g, &mut stack);
    // CR 602.2a: "That ability is created on the stack as an object that's not a card."
    // CR 602.2b: its costs are paid as a spell's are, by the player (CR 601.2h).
    let activating = Who::Player(me, Some(strip));
    assert_eq!(who(&events, arrival), activating);
    assert_eq!(who(&events, |e| matches!(e, Event::Tap { .. })), activating);
    assert_eq!(who(&events, moves(Reason::Sacrifice)), activating);
    assert_eq!(
        who(&events, |e| matches!(e, Event::Ability { .. })),
        activating
    );
    assert!(stack[0].ability.is_some());

    // CR 113.7a: "Once activated or triggered, an ability exists on the stack independently
    // of its source": it resolves and leaves with its source in the graveyard.
    pass(&mut g);
    let events = told(&mut g, &mut stack);
    let effect = Who::Effect(strip);
    assert_eq!(who(&events, moves(Reason::Destroy)), effect);
    assert_eq!(
        who(&events, |e| matches!(e, Event::AbilityFinished)),
        effect
    );
    assert_eq!(who(&events, departure), effect);
    assert!(stack.is_empty());
}

#[test]
fn an_enters_trigger_is_put_on_the_stack_by_its_controller_and_makes_its_token() {
    let mut g = game();
    let me = g.me;
    g.give(me, "Test Land", Zone::Battlefield);
    let hatcher = g.give(me, "Cause Hatcher", Zone::Hand);
    let mut stack = Vec::new();
    g.pick(cast(hatcher));
    told(&mut g, &mut stack);

    // CR 608.3a: the permanent spell "becomes a permanent and enters the battlefield" as it
    // resolves. CR 603.3b: then its controller puts the triggered ability on the stack, where
    // it resolves in turn, neither player having anything to answer it with.
    pass(&mut g);
    let events = told(&mut g, &mut stack);
    let effect = Who::Effect(hatcher);
    assert_eq!(who(&events, moves(Reason::Resolve)), effect);
    let putting = Who::Player(me, Some(hatcher));
    assert_eq!(who(&events, arrival), putting);
    assert_eq!(
        who(&events, |e| matches!(e, Event::Ability { .. })),
        putting
    );
    // The spell left before its ability arrived, so the ability took its place.
    let at = |is: fn(&Event) -> bool| events.iter().position(|(e, _)| is(e)).unwrap();
    assert!(at(departure) < at(arrival));
    let (Event::StackArrival { entry }, _) = events[at(arrival)] else {
        unreachable!("an arrival");
    };
    assert!(entry.ability.is_some());
    assert_eq!(who(&events, moves(Reason::Created)), effect);
    assert_eq!(
        who(&events, |e| matches!(e, Event::TokensCreated { .. })),
        effect
    );
    assert_eq!(who(&events, departure), effect);
    assert_eq!(events.iter().filter(|(e, _)| departure(e)).count(), 2);
    assert!(stack.is_empty());
}

#[test]
fn a_trigger_with_nothing_to_target_arrives_and_leaves() {
    let mut g = game();
    let me = g.me;
    g.give(me, "Test Land", Zone::Battlefield);
    let stinger = g.give(me, "Cause Stinger", Zone::Hand);
    let mut stack = Vec::new();
    g.pick(cast(stinger));
    pass(&mut g);
    let events = told(&mut g, &mut stack);
    // CR 603.3d: "If a choice is required when the triggered ability goes on the stack but
    // no legal choices can be made for it ... the ability is simply removed from the stack."
    let putting = Who::Player(me, Some(stinger));
    let trigger = events.iter().rposition(|(e, _)| arrival(e)).unwrap();
    assert_eq!(events[trigger].1, putting);
    assert_eq!(
        events[trigger + 1],
        (Event::StackDeparture { entry: StackIx(0) }, putting)
    );
    assert!(stack.is_empty() && g.s.stack().is_empty());
}

#[test]
fn a_creature_dealt_lethal_damage_by_a_spell_dies_to_the_rules() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    g.give(me, "Test Land", Zone::Battlefield);
    let shock = g.give(me, "Cause Shock", Zone::Hand);
    let bear = g.give(them, "Cause Bear", Zone::Battlefield);
    let mut stack = Vec::new();
    g.pick(cast(shock));
    pass(&mut g);
    let events = told(&mut g, &mut stack);
    assert_eq!(
        who(&events, |e| matches!(e, Event::Damage { .. })),
        Who::Effect(shock)
    );
    assert_eq!(who(&events, moves(Reason::Resolve)), Who::Effect(shock));
    // CR 704.3: "Whenever a player would get priority ... the game checks for any of the
    // listed conditions for state-based actions, then performs all applicable state-based
    // actions": after the spell has resolved and left, and not as its effect.
    assert_eq!(who(&events, moves(Reason::StateBased)), Who::Rules);
    let left = events.iter().position(|(e, _)| departure(e)).unwrap();
    let died = events
        .iter()
        .position(|(e, _)| moves(Reason::StateBased)(e))
        .unwrap();
    assert!(left < died);
    assert_eq!(g.s.object(bear).zone(), Zone::Graveyard);
}

#[test]
fn combat_and_the_untap_step_are_the_rules_doing() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    let bear = g.give(me, "Cause Bear", Zone::Battlefield);
    let mut stack = Vec::new();
    // Declare the bear as an attacker, and answer every other decision with its first option
    // (a pass, "done", no block), into the attacker's next turn.
    let mut attacked = false;
    while g.s.turn().number() < 3 || g.s.turn().step() != TurnStep::PrecombatMain {
        let d = g.decide();
        let attack = index_of(&d, Opt::Attack(bear)).filter(|_| !attacked);
        attacked |= attack.is_some();
        g.choose(attack.unwrap_or(Choice(0)));
        assert!(
            d.kind != DecisionKind::Blocker,
            "the defender has no creature"
        );
    }
    let events = told(&mut g, &mut stack);
    // CR 508.1: "the active player declares attackers. This turn-based action doesn't use the
    // stack." CR 510.2: "all combat damage that's been assigned is dealt simultaneously. This
    // turn-based action doesn't use the stack." CR 502.3: "the active player determines
    // which permanents they control will untap. Then they untap them all simultaneously.
    // This turn-based action doesn't use the stack."
    assert_eq!(
        who(&events, |e| matches!(e, Event::Attack { .. })),
        Who::Rules
    );
    assert_eq!(who(&events, |e| matches!(e, Event::Tap { .. })), Who::Rules);
    assert_eq!(
        who(&events, |e| matches!(e, Event::Damage { combat: true, .. })),
        Who::Rules
    );
    assert_eq!(
        who(&events, |e| matches!(e, Event::Untap { .. })),
        Who::Rules
    );
    assert_eq!(
        who(&events, |e| matches!(e, Event::Draw { .. })),
        Who::Rules
    );
    assert_eq!(
        who(&events, |e| matches!(e, Event::StepBegins { .. })),
        Who::Rules
    );
    assert_eq!(g.s.player(them).life(), 18);
    assert!(events.iter().all(|&(_, who)| who == Who::Rules));
}

#[test]
fn mana_paid_as_a_spell_resolves_names_that_spell() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    for _ in 0..2 {
        g.give(me, "Test Land", Zone::Battlefield);
    }
    g.give(them, "Test Land", Zone::Battlefield);
    let sage = g.give(me, "Cause Sage", Zone::Hand);
    let tithe = g.give(them, "Cause Tithe", Zone::Hand);
    let mut stack = Vec::new();
    g.pick(cast(sage));
    g.choose(Choice(0));
    g.pick(cast(tithe));
    let events = told(&mut g, &mut stack);
    // The second spell is above the first, and its costs name it.
    assert_eq!(
        who(
            &events,
            |e| matches!(e, Event::Cast { controller, .. } if *controller == them)
        ),
        Who::Player(them, Some(tithe))
    );
    assert_eq!(stack.len(), 2);

    // It resolves, and asks the first spell's controller whether they pay.
    g.pass_both();
    g.pick(Opt::Yes);
    let events = told(&mut g, &mut stack);
    // CR 608.2g: "If an effect gives a player the option to pay mana, they may activate mana
    // abilities before taking that action." CR 605.3a: "even if it's in the middle of ...
    // resolving a spell": the tap is the payer's own doing, the mana the land's effect, and
    // both name the spell that asks, as does the rest of its resolution.
    assert_eq!(
        who(&events, |e| matches!(e, Event::Tap { .. })),
        Who::Player(me, Some(tithe))
    );
    let Who::Object(_, paid_for) = who(&events, |e| matches!(e, Event::Mana { .. })) else {
        panic!("mana a land did not make: {events:?}");
    };
    assert_eq!(paid_for, Some(tithe));
    let effect = Who::Effect(tithe);
    assert_eq!(
        who(&events, |e| matches!(e, Event::ResolutionPayment { .. })),
        effect
    );
    assert_eq!(who(&events, moves(Reason::Resolve)), effect);
    assert_eq!(
        who(&events, departure),
        effect,
        "the spell above left, the one below stays"
    );
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].object.map(|o| o.slot), Some(sage));
}

#[test]
fn a_spell_countered_from_under_the_resolving_one_leaves_first() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    g.give(me, "Test Land", Zone::Battlefield);
    g.give(them, "Test Land", Zone::Battlefield);
    let sage = g.give(me, "Cause Sage", Zone::Hand);
    let deny = g.give(them, "Cause Deny", Zone::Hand);
    let mut stack = Vec::new();
    g.pick(cast(sage));
    g.choose(Choice(0));
    g.pick(cast(deny));
    told(&mut g, &mut stack);
    pass(&mut g);
    let events = told(&mut g, &mut stack);
    // CR 701.6a: "To counter a spell or ability means to cancel it, removing it from the
    // stack. ... A countered spell is put into its owner's graveyard." Both are the effect
    // of the spell that counters, which keeps resolving from the place below as the
    // countered one leaves from under it.
    let effect = Who::Effect(deny);
    assert_eq!(who(&events, moves(Reason::Counter)), effect);
    assert_eq!(who(&events, moves(Reason::Resolve)), effect);
    let departures: Vec<(Event, Who)> = events
        .iter()
        .copied()
        .filter(|(e, _)| departure(e))
        .collect();
    let leaves = Event::StackDeparture { entry: StackIx(0) };
    assert_eq!(departures, [(leaves, effect), (leaves, effect)]);
    assert!(stack.is_empty());
}

#[test]
fn a_run_discarded_at_its_question_leaves_no_event_and_no_tag() {
    let mut g = game();
    let me = g.me;
    g.give(me, "Test Land", Zone::Battlefield);
    g.give(me, "Cause Bear", Zone::Hand);
    let loot = g.give(me, "Cause Loot", Zone::Hand);
    let mut stack = Vec::new();
    g.pick(cast(loot));
    told(&mut g, &mut stack);

    // It draws, then asks which card to discard: the run is discarded with the draw in it
    // (architecture §7.3), and the spell is still on the stack.
    g.pass_both();
    assert!(g.is_paused());
    let events = told(&mut g, &mut stack);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(stack.len(), 1);
    // The view of the question replays the draw, tagged as it will be.
    let seen = paused_events(&g.s, &g.t, &mut g.x);
    let seen = read(&seen, &mut stack.clone());
    assert_eq!(
        who(&seen, |e| matches!(e, Event::Draw { .. })),
        Who::Effect(loot)
    );
    assert!(g.x.events.is_empty() && g.x.events.tags().is_empty());

    // The answer lets it finish: the draw is recorded once, with the discard.
    g.choose(Choice(0));
    let events = told(&mut g, &mut stack);
    let effect = Who::Effect(loot);
    let draws = events
        .iter()
        .filter(|(e, _)| matches!(e, Event::Draw { .. }));
    assert_eq!(draws.count(), 1);
    assert_eq!(who(&events, |e| matches!(e, Event::Draw { .. })), effect);
    assert_eq!(who(&events, moves(Reason::Discard)), effect);
    assert_eq!(who(&events, departure), effect);
    assert!(stack.is_empty());
}

#[test]
fn a_trigger_from_inside_a_resolution_goes_above_what_waits_below() {
    let mut g = game();
    let (me, them) = (g.me, g.them);
    g.give(me, "Test Land", Zone::Battlefield);
    g.give(them, "Test Land", Zone::Battlefield);
    let mourner = g.give(me, "Cause Mourner", Zone::Battlefield);
    let sage = g.give(me, "Cause Sage", Zone::Hand);
    let doom = g.give(them, "Cause Doom", Zone::Hand);
    let mut stack = Vec::new();
    g.pick(cast(sage));
    g.choose(Choice(0));
    g.pick(cast(doom));
    told(&mut g, &mut stack);
    assert_eq!(stack.len(), 2);

    // The creature dies as the instant above resolves, so its ability triggers then. CR
    // 603.3: "Once an ability has triggered, its controller puts it on the stack as an object
    // that's not a card the next time a player would receive priority": after the instant
    // has left, on top of the sorcery still waiting, and as its controller's doing, not the
    // instant's effect.
    g.pass_both();
    let events = told(&mut g, &mut stack);
    let effect = Who::Effect(doom);
    assert_eq!(who(&events, moves(Reason::Destroy)), effect);
    assert_eq!(who(&events, departure), effect);
    let putting = Who::Player(me, Some(mourner));
    assert_eq!(who(&events, arrival), putting);
    let at = |is: fn(&Event) -> bool| events.iter().position(|(e, _)| is(e)).unwrap();
    assert!(at(departure) < at(arrival));
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[0].object.map(|o| o.slot), Some(sage));
    assert!(stack[1].ability.is_some());

    // The trigger resolves before the sorcery under it: its draw and its leaving are its
    // own, and the sorcery stays.
    g.pass_both();
    let events = told(&mut g, &mut stack);
    let trigger = Who::Effect(mourner);
    assert_eq!(who(&events, |e| matches!(e, Event::Draw { .. })), trigger);
    assert_eq!(
        events.iter().find(|(e, _)| departure(e)).copied(),
        Some((Event::StackDeparture { entry: StackIx(1) }, trigger))
    );
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].object.map(|o| o.slot), Some(sage));
}
