//! Recorded fixtures played from end to end through the client, with the timings the session
//! sent: `verbs-zones`, whose scripts move cards between every zone, and the stress boards.

use std::time::Instant;

use beryl_inspect::pages::{Recorded, check_recording, recording};
use beryl_inspect::stress;
use beryl_scene::agent::Layout;
use beryl_scene::driver::Page;
use beryl_scene::frame::{Frame, FrameBuilder, Wake};
use beryl_scene::input::{Insets, Size};
use beryl_scene::play::Show;
use beryl_scene::table::{DESKTOP, Table, Window};
use beryl_session::protocol::Side;
use beryl_session::script::Verb;

const SEATS: [Side; 2] = [Side::You, Side::Opponent];

/// The verbs that have motion of their own; every other verb is drawn as an instant change.
const BUILT: [Verb; 3] = [Verb::Fly, Verb::Appear, Verb::Set];

#[test]
fn verbs_zones_plays_to_its_end_from_both_seats() {
    for seat in SEATS {
        let mut page = Page::fixture("verbs-zones", seat, Layout::Desktop);
        page.seek(&"end".parse().expect("a moment"))
            .expect("the recording has an end");
        page.still();
        assert!(page.ended(), "{seat:?}: every view was given");
        // Several Islands are on the table; the first the tree lists names its own.
        let tree = page.tree();
        let line = tree
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("card \"Island\" #"))
            .unwrap_or_else(|| panic!("{seat:?}: no Island in\n{tree}"));
        let locator: String = line
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(page.find(&locator).name, "Island", "{seat:?}");
        let mut unbuilt = Vec::new();
        while let Some(verb) = page.next_unbuilt() {
            unbuilt.push(verb);
        }
        for verb in BUILT {
            assert!(
                !unbuilt.contains(&verb),
                "{seat:?}: {verb:?} is built, so it animates"
            );
        }
        println!("{seat:?}: played as instant changes: {unbuilt:?}");
    }
}

#[test]
fn after_each_verbs_zones_script_every_object_rests_at_its_placement() {
    for seat in SEATS {
        let views = recording("verbs-zones", seat)
            .and_then(|recording| recording.views())
            .unwrap_or_else(|why| panic!("{why}"));
        let window = Window {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        };
        let mut show = Show::new(Table::new(DESKTOP, window));
        let mut frame = Frame::with_capacity(4096, 8192, 65_536);
        let mut now = 0;
        for (index, update) in views.into_iter().enumerate() {
            show.push(Box::new(update.clone()), timings(&update));
            show.frame(now, &mut FrameBuilder::new(&mut frame));
            let placement = show.table().placement(&update.view);
            // Far past the script's end, each moving object is where its track ends.
            let ended = u64::MAX / 4;
            // An object that leaves for a hidden zone ends at that zone's pile and then has no
            // placement; the check after the script covers it.
            for object in &update.view.objects {
                let Some(rest) = placement.get(object.slot).map(|rest| rest.pose) else {
                    continue;
                };
                assert_eq!(
                    show.pose(usize::from(object.slot.0), ended),
                    Some(rest),
                    "{seat:?} view {}: card #{} ends its script at its placement",
                    index + 1,
                    object.slot.0
                );
            }
            loop {
                now = match show.wake(now) {
                    Wake::Never => break,
                    Wake::Now => now + 16_666_667,
                    Wake::At(at) => at.max(now + 1),
                };
                show.frame(now, &mut FrameBuilder::new(&mut frame));
            }
            for object in &update.view.objects {
                let rest = placement.get(object.slot).map(|rest| rest.pose);
                assert_eq!(
                    show.pose(usize::from(object.slot.0), now),
                    rest,
                    "{seat:?} view {}: card #{} rests at its placement",
                    index + 1,
                    object.slot.0
                );
            }
        }
    }
}

#[test]
fn verbs_zones_passes_every_check_on_every_tick() {
    let started = Instant::now();
    let checked = check_recording("verbs-zones");
    println!(
        "check_recording(\"verbs-zones\"): {} plays, {} frames, {} ms",
        checked.plays,
        checked.frames,
        started.elapsed().as_millis()
    );
    assert_eq!(checked.plays, 8, "both seats in four windows");
}

#[test]
fn the_stress_boards_play_to_their_end() {
    for name in stress::NAMES {
        let mut page = Page::fixture(name, Side::You, Layout::Desktop);
        page.seek(&"end".parse().expect("a moment"))
            .expect("the board has an end");
        page.still();
        assert!(page.ended(), "{name}");
        let mut unbuilt = Vec::new();
        while let Some(verb) = page.next_unbuilt() {
            unbuilt.push(verb);
        }
        assert_eq!(unbuilt, [], "{name}: its flights animate");
    }
}

/// The timings the session sent with `update`'s script, which the client plays.
fn timings(update: &beryl_session::GameUpdate) -> Vec<beryl_session::script::StepTiming> {
    update
        .view
        .script
        .as_ref()
        .map(|script| script.timing.clone())
        .unwrap_or_default()
}
