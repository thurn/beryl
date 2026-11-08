//! What the views' events say of an ability put on the stack, for the game log: its source,
//! its loyalty cost and its targets, as the stack showed them when it was activated
//! ([CR 602.2b](https://yawgatog.com/resources/magic-rules/#R6022b)): "The remainder of the
//! process for activating an ability is identical to the process for casting a spell listed in
//! rules 601.2b-i." It may have resolved by the time a view shows.

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::{Choice, DecisionKind, Observation, Replay, Scratch, Seat, Step, apply, next};
use beryl_session::StopsSetting;
use beryl_session::protocol::{GameEvent, Moment, Side, StackItem, TargetRef, player_view};

/// The events of `card`'s scenario named `name` as its first `actions` actions play out, and
/// the decisions with one answer after them, as `viewer` is told them; and the slot of each
/// card on the battlefield then, by name.
fn events(
    card: &str,
    name: &str,
    actions: usize,
    viewer: Seat,
) -> (Vec<GameEvent>, impl Fn(&str) -> u16) {
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(c, _)| c == card)
        .expect("the card's scenarios");
    let scenario = scenarios
        .iter()
        .find(|s| s.name == name)
        .expect("the scenario");
    let (t, mut s) = beryl_scenario::position(&CARDS, scenario).unwrap_or_else(|e| panic!("{e}"));
    let mut x = Scratch::recording();
    let decide = |s: &mut beryl_engine::State, x: &mut Scratch| match next(s, &t, x, &mut Replay) {
        Step::Decide(d) => d,
        Step::Over(o) => panic!("the game ended: {o:?}"),
    };
    for action in &scenario.actions[..actions] {
        loop {
            let d = decide(&mut s, &mut x);
            if let Some(c) = beryl_scenario::answer(&t, &s, &d, action) {
                apply(&mut s, &t, &mut x, &mut Replay, c);
                break;
            }
            assert_eq!(d.len(), 1, "{}", beryl_scenario::describe(&t, &s, &d));
            apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
        }
    }
    loop {
        let d = decide(&mut s, &mut x);
        if d.len() != 1 || d.kind == DecisionKind::Priority {
            break;
        }
        apply(&mut s, &t, &mut x, &mut Replay, Choice(0));
    }
    let view = player_view(
        &t,
        Observation::new(&s, viewer),
        Moment::Thinking(None),
        StopsSetting::SMART,
        x.events.events(),
    );
    let slots: Vec<(String, u16)> = view
        .objects
        .iter()
        .filter_map(|o| Some((o.name.clone()?, o.slot)))
        .collect();
    let slot = move |name: &str| {
        slots
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} is seen"))
            .1
    };
    (view.events, slot)
}

/// The abilities among `events`, with their loyalty costs.
fn abilities(events: &[GameEvent]) -> Vec<(&StackItem, Option<i8>)> {
    events
        .iter()
        .filter_map(|e| match e {
            GameEvent::Ability { item, loyalty } => Some((item, *loyalty)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_loyalty_activation_names_its_source_cost_and_target_player() {
    // Jace's +1 targeting P1, as P1 sees it.
    let (events, slot) = events(
        names::JACE_WIELDER_OF_MYSTERIES,
        "+1 mills the target player two, then draws",
        2,
        Seat::new(1),
    );
    let jace = slot(names::JACE_WIELDER_OF_MYSTERIES);
    let seen = abilities(&events);
    let [(item, loyalty)] = seen.as_slice() else {
        panic!("one ability: {events:?}");
    };
    assert_eq!(*loyalty, Some(1));
    let StackItem::Ability {
        source,
        name,
        triggered,
        controller,
        targets,
        ..
    } = item
    else {
        panic!("an ability: {item:?}");
    };
    assert_eq!(*source, Some(jace));
    assert_eq!(name, names::JACE_WIELDER_OF_MYSTERIES);
    assert!(!triggered);
    assert_eq!(*controller, Side::Opponent);
    assert_eq!(targets, &[TargetRef::Player { player: Side::You }]);
    // The loyalty counter paid comes before it, for the log to fold into its line.
    let at = events
        .iter()
        .position(|e| matches!(e, GameEvent::Ability { .. }))
        .expect("the ability");
    assert!(
        events[..at].iter().any(|e| matches!(
            e,
            GameEvent::CountersAdded { slot, amount: 1, .. } if *slot == jace
        )),
        "{events:?}"
    );
}

#[test]
fn a_targeted_ability_names_its_source_cost_and_target_object() {
    // Teferi's −3 targeting P1's Staff, as P0 sees it.
    let (events, slot) = events(
        names::TEFERI_TIME_RAVELER,
        "−3 returns an opposing artifact and draws a card",
        2,
        Seat::new(0),
    );
    let teferi = slot(names::TEFERI_TIME_RAVELER);
    let seen = abilities(&events);
    let [(item, loyalty)] = seen.as_slice() else {
        panic!("one ability: {events:?}");
    };
    assert_eq!(*loyalty, Some(-3));
    let StackItem::Ability {
        source,
        controller,
        targets,
        ..
    } = item
    else {
        panic!("an ability: {item:?}");
    };
    assert_eq!(*source, Some(teferi));
    assert_eq!(*controller, Side::You);
    let [TargetRef::Object { .. }] = targets.as_slice() else {
        panic!("one object targeted: {targets:?}");
    };
}

#[test]
fn a_cast_names_its_spell_and_targets_as_the_stack_showed_them() {
    // Oust aimed at P1's creature, as P1 sees it: a spell that resolves unanswered is gone from
    // the stack by the next view, which shows its cast from the event alone.
    let (events, slot) = events(
        names::OUST,
        "Oust puts the creature second from the top and its controller gains 3",
        1,
        Seat::new(1),
    );
    let oust = slot(names::OUST);
    let creature = slot(names::ELITE_SPELLBINDER);
    let casts: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            GameEvent::Cast {
                slot,
                controller,
                item,
            } => Some((*slot, *controller, item)),
            _ => None,
        })
        .collect();
    let [(spell, controller, item)] = casts.as_slice() else {
        panic!("one cast: {events:?}");
    };
    assert_eq!((*spell, *controller), (oust, Side::Opponent));
    let StackItem::Spell { slot, targets, .. } = item else {
        panic!("a spell: {item:?}");
    };
    assert_eq!(*slot, oust);
    assert_eq!(targets, &[TargetRef::Object { slot: creature }]);
}
