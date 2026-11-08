//! What a face-down permanent shows each player (architecture §13): to both a 2/2 creature with
//! no name (CR 708.2a), and to its controller alone the card it is
//! ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "At any time, you may look
//! at a face-down spell you control on the stack or a face-down permanent you control ... You
//! can't look at face-down cards in any other zone or face-down spells or permanents controlled
//! by another player."

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::{Observation, Seat, SlotId, Zone};
use beryl_session::StopsSetting;
use beryl_session::protocol::{
    CardType, Facing, Moment, ObjectView, PowerToughness, Side, player_view,
};

/// Abhorrent Oculus's scenario named `name`.
fn oculus_scenario(name: &str) -> &'static beryl_scenario::Scenario {
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(card, _)| card == names::ABHORRENT_OCULUS)
        .expect("the card's scenarios");
    scenarios
        .iter()
        .find(|s| s.name == name)
        .expect("the scenario")
}

/// The face-down permanent each player sees once Abhorrent Oculus has manifested Snapcaster
/// Mage, as player 0 (its controller) and player 1.
fn seen() -> [ObjectView; 2] {
    let scenario =
        oculus_scenario("Abhorrent Oculus manifests dread on each opponent's upkeep only");
    let mut seen = None;
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| {
        let view = |viewer| {
            let v = player_view(
                t,
                Observation::new(s, viewer),
                Moment::Thinking(None),
                StopsSetting::SMART,
                &[],
            );
            v.objects.into_iter().find(|o| o.facing == Facing::Down)
        };
        if let (Some(mine), Some(theirs)) = (view(Seat::P0), view(Seat::P1)) {
            seen = Some([mine, theirs]);
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    seen.expect("a face-down permanent")
}

#[test]
fn a_face_down_permanent_is_a_nameless_two_two_to_its_opponent() {
    let [mine, theirs] = seen();
    let two_two = Some(PowerToughness {
        power: 2,
        toughness: 2,
    });
    assert_eq!(mine.slot, theirs.slot);
    assert_eq!(mine.name.as_deref(), Some(names::SNAPCASTER_MAGE));
    assert_eq!(theirs.name, None);
    assert_eq!(theirs.controller, Side::Opponent);
    for o in [&mine, &theirs] {
        assert_eq!(o.pt, two_two);
        assert_eq!(o.types, [CardType::Creature]);
    }
}

/// Fractured Identity's copy of the face-down Snapcaster Mage is a face-up token, since being
/// face down is a status a copy doesn't copy
/// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)), of the face-down values
/// ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)): "Any listed characteristics
/// are the copiable values of that object's characteristics." Both players see the same
/// nameless 2/2, and neither the state nor either view of it holds Snapcaster Mage.
#[test]
fn a_copy_of_a_face_down_permanent_is_a_face_up_nameless_two_two() {
    let scenario = oculus_scenario("Fractured Identity copies a face-down permanent as a 2/2");
    let mut last = None;
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| last = Some((t.clone(), *s)))
        .unwrap_or_else(|e| panic!("{e}"));
    let (t, s) = last.expect("a decision");
    let copy = s
        .battlefield()
        .iter()
        .copied()
        .find(|&slot| {
            s.object(slot)
                .flags
                .has(beryl_engine::state::ObjectFlags::TOKEN)
        })
        .expect("the copy");
    let snapcaster = t
        .index_of(CARDS.get(names::SNAPCASTER_MAGE).unwrap())
        .unwrap();
    assert_eq!(s.card(copy), t.face_down_copy());
    assert_ne!(s.card(copy), snapcaster);
    for viewer in Seat::BOTH {
        let obs = Observation::new(&s, viewer);
        assert_eq!(obs.card(copy), Some(t.face_down_copy()), "{viewer:?}");
        let view = player_view(&t, obs, Moment::Thinking(None), StopsSetting::SMART, &[]);
        let o = view
            .objects
            .iter()
            .find(|o| o.slot == copy.num())
            .expect("the copy's view");
        assert_eq!(o.zone, Zone::Battlefield.into());
        assert_eq!(o.facing, Facing::Up, "{viewer:?}");
        assert!(o.token);
        assert_eq!(o.name.as_deref(), Some(""), "{viewer:?}");
        assert_eq!(
            o.pt,
            Some(PowerToughness {
                power: 2,
                toughness: 2
            })
        );
        assert_eq!(o.types, [CardType::Creature]);
    }
}
