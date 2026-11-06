//! What a face-down permanent shows each player (architecture §13): to both a 2/2 creature with
//! no name (CR 708.2a), and to its controller alone the card it is
//! ([CR 708.5](https://yawgatog.com/resources/magic-rules/#R7085)): "At any time, you may look
//! at a face-down spell you control on the stack or a face-down permanent you control ... You
//! can't look at face-down cards in any other zone or face-down spells or permanents controlled
//! by another player."

use beryl_cards::{CARDS, SCENARIOS, names};
use beryl_engine::{Observation, Seat};
use beryl_session::StopsSetting;
use beryl_session::protocol::{
    CardType, Facing, Moment, ObjectView, PowerToughness, Side, player_view,
};

/// The face-down permanent each player sees once Abhorrent Oculus has manifested Snapcaster
/// Mage, as player 0 (its controller) and player 1.
fn seen() -> [ObjectView; 2] {
    let (_, scenarios) = SCENARIOS
        .iter()
        .find(|&&(name, _)| name == names::ABHORRENT_OCULUS)
        .expect("the card's scenarios");
    let scenario = scenarios
        .iter()
        .find(|s| s.name == "Abhorrent Oculus manifests dread on each opponent's upkeep only")
        .expect("the manifest scenario");
    let mut seen = None;
    beryl_scenario::visit(&CARDS, scenario, |t, s, _| {
        let view = |viewer| {
            let v = player_view(
                t,
                Observation::new(s, viewer),
                Moment::Thinking,
                StopsSetting::Smart,
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
