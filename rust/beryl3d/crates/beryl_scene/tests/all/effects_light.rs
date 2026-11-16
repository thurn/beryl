//! The effects that are one record of light each, `glow`, `impact`, `flash`, `beam`, `pulse`
//! and `trail` (docs/beryl-3d.md, "Effects"): each is drawn while its run is under way and at
//! no other time, where its card is on that frame, and changes nothing else on the table.

use beryl_protocol::script::{Curve, Edge, StepEffect, StepTiming, Subject};
use beryl_scene::frame::{Frame, Material};
use beryl_scene::play::Show;
use beryl_scene::table::{DESKTOP, Table};
use glam::Vec3;

use crate::common::{MS, before, discard, empty_frame, frame, window};

/// The six effects this file covers.
const EFFECTS: [&str; 6] = ["glow", "impact", "flash", "beam", "pulse", "trail"];

/// An effect named `name` on the step's subject from a quarter of the way through its step,
/// for 150 ms: from 200 to 350 ms in the script of `started`, while the card is in flight.
fn effect(name: &str) -> StepEffect {
    StepEffect {
        name: name.to_owned(),
        when: Edge::Start,
        on: None,
        into: Some(0.25),
        duration: Some(150.0),
        delay: 0.0,
        curve: Curve::Linear,
    }
}

/// A show at 1600 by 900 whose first script shows the table and whose second discards a card
/// in one step from 100 to 500 ms that begins `effects`, both started by the frame at 0, so
/// that each script time is a real time.
fn started(effects: Vec<StepEffect>) -> Show {
    let (start, cards) = before();
    let (after, mut timings) = discard(&start, cards);
    timings[0] = StepTiming {
        delay: 100.0,
        effects,
        ..timings[0].clone()
    };
    let mut show = Show::new(Table::new(DESKTOP, window(1600.0, 900.0)));
    show.push(Box::new(start), Vec::new());
    show.push(Box::new(after), timings);
    let mut buffer = empty_frame();
    frame(&mut show, &mut buffer, 0);
    show
}

/// The records of light (glows and streaks) in `frame`.
fn lights(frame: &Frame) -> Vec<[f32; 12]> {
    frame
        .draws
        .iter()
        .filter(|draw| {
            draw.material == Material::Glow as u8 || draw.material == Material::Streak as u8
        })
        .map(|draw| draw.model)
        .collect()
}

/// The middle of the shape a record's transform draws.
fn centre(model: &[f32; 12]) -> Vec3 {
    Vec3::new(model[3], model[7], model[11])
}

/// The frames of a show with `effects` and of the same show without them, each at the same
/// times from 0 to 600 ms.
fn both(effects: Vec<StepEffect>) -> Vec<(u64, Frame, Frame)> {
    let mut with = started(effects);
    let mut without = started(Vec::new());
    let (mut a, mut b) = (empty_frame(), empty_frame());
    (0..=60)
        .map(|tick| {
            let now = tick * 10 * MS;
            (
                now,
                frame(&mut with, &mut a, now),
                frame(&mut without, &mut b, now),
            )
        })
        .collect()
}

#[test]
fn each_effect_is_one_record_while_its_run_plays_and_none_outside_it() {
    for name in EFFECTS {
        for (now, with, without) in both(vec![effect(name)]) {
            let under_way = (200 * MS..350 * MS).contains(&now);
            assert_eq!(
                lights(&with).len(),
                lights(&without).len() + usize::from(under_way),
                "`{name}` at {} ms",
                now / MS
            );
        }
    }
}

#[test]
fn a_light_follows_its_card_in_flight() {
    let mut show = started(vec![effect("glow"), effect("impact")]);
    let mut buffer = empty_frame();
    let mut centres = Vec::new();
    for now in [210 * MS, 270 * MS, 340 * MS] {
        let frame = frame(&mut show, &mut buffer, now);
        let card = show.effects()[0].on;
        let reach = show.reach(card, now).expect("the card is on the table");
        for model in lights(&frame) {
            let off = centre(&model) - reach.pose.position;
            assert!(
                off.length() < 0.5,
                "the light lies on the card at {} ms, {off} away",
                now / MS
            );
        }
        centres.push(centre(&lights(&frame)[0]));
    }
    assert!(
        centres[0].distance(centres[2]) > 10.0,
        "the card moved, and its light with it: {centres:?}"
    );
}

#[test]
fn a_light_lies_just_over_its_card() {
    // A card lies on the table itself, so even the glow the web draws under it lies over it,
    // where the depth test can tell it from the table.
    for name in ["glow", "impact", "flash", "pulse"] {
        let mut show = started(vec![effect(name)]);
        let mut buffer = empty_frame();
        let now = 250 * MS;
        let frame = frame(&mut show, &mut buffer, now);
        let card = show.effects()[0].on;
        let height = show
            .reach(card, now)
            .expect("the card is on the table")
            .pose
            .position
            .z;
        let light = centre(&lights(&frame)[0]).z;
        assert!(
            light > height && light < height + 1.0,
            "`{name}` at {light}, its card at {height}"
        );
    }
}

#[test]
fn an_effect_moves_nothing() {
    for (now, with, without) in both(EFFECTS.iter().map(|name| effect(name)).collect()) {
        let corners = |frame: &Frame| {
            frame
                .resolved
                .iter()
                .map(|element| (element.id, element.corners))
                .collect::<Vec<_>>()
        };
        assert_eq!(corners(&with), corners(&without), "at {} ms", now / MS);
    }
}

#[test]
fn one_glow_lights_little_of_the_screen() {
    // On a creature at rest on the battlefield, the size most glows are drawn at.
    let angel = before().1.angel;
    let frames = both(vec![StepEffect {
        on: Some(Subject::Object { slot: angel }),
        ..effect("glow")
    }]);
    // The largest it is drawn, as it ends, grown by its spread.
    let area = frames
        .iter()
        .map(|(_, with, without)| with.load.blended_area - without.load.blended_area)
        .fold(0.0, f32::max);
    assert!(area > 0.0, "the glow is drawn");
    assert!(area < 0.02, "a glow covers {area} screens");
}

#[test]
fn a_beam_runs_from_the_side_of_its_card_toward_its_left() {
    let mut show = started(vec![effect("beam")]);
    let mut buffer = empty_frame();
    let now = 300 * MS;
    let frame = frame(&mut show, &mut buffer, now);
    let card = show.effects()[0].on;
    let pose = show
        .reach(card, now)
        .expect("the card is on the table")
        .pose;
    let model = lights(&frame)[0];
    // The cone sprite opens up its cell, along the quad's own y axis.
    let along = Vec3::new(model[1], model[5], model[9]).normalize();
    let left = pose.rotation * Vec3::NEG_X;
    assert!(
        along.dot(left) > 0.99,
        "the beam runs along {along}, not {left}"
    );
    let from = centre(&model) - along * Vec3::new(model[1], model[5], model[9]).length() * 0.5;
    assert!(
        (from - pose.position).dot(left) > 0.0,
        "its point is on the card's left side"
    );
}
