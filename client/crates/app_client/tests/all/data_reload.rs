//! Runtime tuning takes effect during playback and at rest.

#[path = "../../../../test_profiles.rs"]
mod test_profiles;

use test_profiles::SHIPPED;

use std::sync::Arc;

use app_client::{Client, Config, Input};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectId, Why, ZoneName};
use beryl_protocol::script::Curve;
use kit_base::data::profiles;
use kit_base::pose::Segment;
use kit_frame::frame::{PaintBand, Role};

use super::{flown, table, timed};
use crate::common::MS;

#[test]
fn reloading_during_a_flight_uses_new_motion_and_reaches_the_new_layout() {
    let (before, [mine, _]) = table();
    let discard = |before: &GameUpdate, slot| {
        let mut after = flown(
            before,
            slot,
            ZoneName::Hand,
            ZoneName::Graveyard,
            Why::Discard,
        );
        after.view.script.as_mut().unwrap().timing =
            vec![timed("discard", 420.0, 0.0, Curve::EaseOut, true)];
        after
    };
    let after = discard(&before, mine.spell);
    let queued = discard(&after, mine.other);

    let old = crate::test_data::data();
    let mut new = (*old).clone();
    new.verbs.fly.arc += 100.0;
    let (name, contents) = SHIPPED[1];
    let mut profile: serde_json::Value = serde_json::from_str(contents).unwrap();
    profile["table"]["camera"]["pitch_degrees"] = serde_json::json!(34.0);
    let profile = profile.to_string();
    new.profiles = profiles::Profiles::parse(&[SHIPPED[0], (name, &profile)], new.card).unwrap();
    let new = Arc::new(new);

    let client = |data| {
        let mut client = Client::new(data, Config::LARGEST);
        client.record();
        client.handle(Input::Update(Box::new(before.clone())));
        client.frame(0);
        client.handle(Input::Update(Box::new(after.clone())));
        client.handle(Input::Update(Box::new(queued.clone())));
        client.frame(MS);
        client
    };
    let mut reloaded = client(Arc::clone(&old));
    let mut unchanged = client(old);
    let mut fresh = client(Arc::clone(&new));
    let card_at = |client: &mut Client, at, slot: ObjectId, band| {
        let frame = client.frame(at);
        let card = frame
            .resolved
            .iter()
            .find(|card| {
                card.role == Role::Card && card.id.0 == u32::from(slot.0) && card.pose.is_some()
            })
            .expect("the card is placed");
        assert_eq!(PaintBand::of(card), band);
        (card.pose, card.corners)
    };
    let midflight = 170 * MS;
    card_at(&mut reloaded, midflight, mine.spell, PaintBand::Moving);
    reloaded.handle(Input::DataError("verbs.json: bad arc".to_owned()));
    assert_eq!(
        card_at(&mut reloaded, midflight, mine.spell, PaintBand::Moving),
        card_at(&mut unchanged, midflight, mine.spell, PaintBand::Moving),
    );
    let has_notice = |client: &mut Client| {
        let frame = client.frame(midflight);
        frame
            .resolved
            .iter()
            .any(|node| frame.name(node.name) == "Data reload failed")
    };
    assert!(has_notice(&mut reloaded));
    reloaded.handle(Input::Data(new));
    assert!(!has_notice(&mut reloaded));
    // Resize keeps the pose at the instant it retargets the active flight. Its new endpoint
    // is the one the same flight would have had with the new tuning from the start.
    assert_eq!(
        flight(&reloaded, 1, mine.spell).to,
        flight(&fresh, 1, mine.spell).to
    );
    assert_ne!(
        flight(&reloaded, 1, mine.spell).to,
        flight(&unchanged, 1, mine.spell).to
    );
    for at in [450 * MS, 900 * MS, 1200 * MS] {
        for client in [&mut reloaded, &mut fresh, &mut unchanged] {
            client.frame(at);
        }
    }
    assert_eq!(
        flight(&reloaded, 2, mine.other),
        flight(&fresh, 2, mine.other)
    );
    assert_ne!(
        flight(&reloaded, 2, mine.other).arc.to_bits(),
        flight(&unchanged, 2, mine.other).arc.to_bits()
    );
    let rest = |client: &mut Client| {
        client
            .frame(1500 * MS)
            .resolved
            .iter()
            .filter_map(|card| card.pose.map(|pose| (card.id, pose, card.corners)))
            .collect::<Vec<_>>()
    };
    assert_eq!(rest(&mut reloaded), rest(&mut fresh));
    assert_ne!(rest(&mut reloaded), rest(&mut unchanged));
}

fn flight(client: &Client, script: usize, slot: ObjectId) -> Segment {
    client.history()[script]
        .segments
        .iter()
        .filter(|track| track.slot == usize::from(slot.0))
        .max_by_key(|track| track.segment.duration)
        .expect("the flight has a track")
        .segment
}
