//! The sounds of a script's steps (docs/beryl-3d.md, "Sound"): each sound a step asks for is in
//! the first frame that reaches its time and in no other frame, a sound of the opponent's doing
//! says so, two of the same sound at one time are one, and the frame rate does not change which
//! sounds are listed.

use crate::common::{MS, empty_frame, frame, show};
use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{ObjectView, Side, Why, ZoneName};
use beryl_protocol::script::{Act, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, step};
use game_play::sound::NAMES;

/// The sounds of the frames that list any, as the frame's time in nanoseconds and each sound's
/// name and whether it is the opponent's.
type Listed = Vec<(u64, Vec<(&'static str, bool)>)>;

/// The sample table, a script of one beat for each of `acts` played over it, and the frames
/// from 1 ms to 2 s, `every` nanoseconds apart, as the sounds each lists, with the script's steps
/// as they played, as their start and end in nanoseconds. A step is in the beat of the step
/// before it where `with` says so.
fn play(
    acts: impl FnOnce(&GameUpdate) -> Vec<(Act, bool)>,
    every: u64,
) -> (Listed, Vec<(u64, u64)>) {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let acts = acts(&start);
    let mut script = ScriptBuilder::default().build();
    for (act, with) in &acts {
        let step = step(act.clone());
        match script.beats.last_mut() {
            Some(beat) if *with => beat.push(step),
            _ => script.beats.push(vec![step]),
        }
    }
    let mut timings = time_script(&script, &Played::of(&start.view));
    for (timing, (_, with)) in timings.iter_mut().zip(&acts) {
        timing.with = *with;
    }
    let mut after = start.clone();
    after.view.script = Some(script);
    let mut show = show();
    show.push(Box::new(start), Vec::new());
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 0);
    show.record();
    show.push(Box::new(after), timings);
    let mut listed = Vec::new();
    let mut now = 0;
    while now <= 2_000 * MS {
        let drawn = frame(&mut show, &mut scratch, now);
        if !drawn.sounds.is_empty() {
            let names = drawn
                .sounds
                .iter()
                .map(|sound| (NAMES[usize::from(sound.id.0)], sound.theirs))
                .collect();
            listed.push((now, names));
        }
        now += every;
    }
    let record = &show.history()[0];
    let begun = record.start.expect("the script started");
    let steps = (record.steps.iter())
        .map(|step| (begun + step.from, begun + step.to))
        .collect();
    (listed, steps)
}

/// The record of the card in `zone` of `side` that is `nth` of those, as a card that has moved
/// to `to`.
fn moved(start: &GameUpdate, side: Side, zone: ZoneName, nth: usize, to: ZoneName) -> ObjectView {
    let mut object = (start.view.objects.iter())
        .filter(|object| object.zone == zone && object.controller == side)
        .nth(nth)
        .expect("the sample table has the card")
        .clone();
    object.zone = to;
    object
}

fn fly(from: ZoneName, why: Why, object: ObjectView) -> Act {
    Act::Fly {
        from,
        why,
        object,
        at: None,
        item: None,
        origin: None,
    }
}

/// The first frame at or after `at` that frames `every` apart include, in nanoseconds.
fn reached(at: u64, every: u64) -> u64 {
    at.div_ceil(every) * every
}

#[test]
fn the_list_of_names_is_the_manifests() {
    let json: serde_json::Value = serde_json::from_str(
        &beryl_test_files::read_to_string("ui/src/audio/manifest.json")
            .expect("the manifest exists"),
    )
    .expect("the manifest is JSON");
    let sounds = json["sounds"].as_object().expect("`sounds` is an object");
    assert_eq!(NAMES.len(), sounds.len(), "one name for each sound");
    for name in NAMES {
        assert!(sounds.contains_key(name), "the manifest has {name}");
    }
    assert_eq!(NAMES[0], "click", "in the order the file lists them");
}

#[test]
fn a_land_played_sounds_as_it_lands_and_a_tap_as_it_begins() {
    let (listed, steps) = play(
        |start| {
            let mut island = moved(
                start,
                Side::You,
                ZoneName::Battlefield,
                0,
                ZoneName::Battlefield,
            );
            island.tapped = true;
            vec![
                (
                    fly(
                        ZoneName::Hand,
                        Why::Play,
                        moved(start, Side::You, ZoneName::Hand, 1, ZoneName::Battlefield),
                    ),
                    false,
                ),
                (Act::Tap { object: island }, false),
            ]
        },
        MS,
    );
    // A card is placed as its flight ends; a tap sounds as it begins.
    let expected = [(steps[0].1, "place"), (steps[1].0, "tap")];
    // The land lands as the tap begins, so one frame lists both.
    assert_eq!(expected[0].0, expected[1].0);
    assert_eq!(
        listed,
        [(
            reached(expected[0].0, MS),
            vec![("place", false), ("tap", false)]
        )]
    );
}

#[test]
fn a_spell_the_opponent_casts_is_theirs() {
    let (listed, steps) = play(
        |start| {
            let spell = moved(start, Side::Opponent, ZoneName::Hand, 0, ZoneName::Stack);
            vec![(fly(ZoneName::Hand, Why::Cast, spell), false)]
        },
        MS,
    );
    assert_eq!(
        listed,
        [(reached(steps[0].0, MS), vec![("cast", true)])],
        "a cast sounds as it begins, quieter"
    );
}

#[test]
fn two_cards_drawn_at_one_instant_sound_once() {
    let (listed, steps) = play(
        |start| {
            let draw = |nth| {
                fly(
                    ZoneName::Library,
                    Why::Draw,
                    moved(start, Side::You, ZoneName::Library, nth, ZoneName::Hand),
                )
            };
            vec![(draw(0), false), (draw(1), true)]
        },
        MS,
    );
    assert_eq!(steps[0].0, steps[1].0, "the two begin together");
    assert_eq!(
        listed,
        [(reached(steps[0].0, MS), vec![("slide", false)])],
        "one slide"
    );
}

#[test]
fn the_frame_rate_does_not_change_which_sounds_are_listed() {
    let acts = |start: &GameUpdate| {
        let mut island = moved(
            start,
            Side::You,
            ZoneName::Battlefield,
            0,
            ZoneName::Battlefield,
        );
        island.tapped = true;
        vec![
            (
                fly(
                    ZoneName::Hand,
                    Why::Play,
                    moved(start, Side::You, ZoneName::Hand, 1, ZoneName::Battlefield),
                ),
                false,
            ),
            (
                fly(
                    ZoneName::Hand,
                    Why::Discard,
                    moved(start, Side::You, ZoneName::Hand, 0, ZoneName::Graveyard),
                ),
                false,
            ),
            (Act::Tap { object: island }, false),
        ]
    };
    let flat = |listed: Listed| -> Vec<(&'static str, bool)> {
        listed.into_iter().flat_map(|(_, names)| names).collect()
    };
    let sixty = flat(play(acts, 1_000_000_000 / 60).0);
    let thirty = flat(play(acts, 1_000_000_000 / 30).0);
    assert_eq!(sixty.len(), 3, "each step sounds once");
    assert_eq!(sixty, thirty);
}
