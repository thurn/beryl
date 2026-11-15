//! Recorded fixtures played from end to end through the client, with the timings the session
//! sent: `verbs-zones`, whose scripts move cards between every zone, `pay-default`, whose
//! prompts are answered by pressing the flat layer's buttons, and the stress boards.

use std::time::Instant;

use beryl_inspect::pages::{Recorded, check_recording, recording};
use beryl_inspect::{recordings, stress};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use beryl_protocol::script::Verb;
use beryl_scene::agent::tree::line;
use beryl_scene::agent::{Layout, locate};
use beryl_scene::driver::{Duration, Moment, Page, TICK};
use beryl_scene::frame::{Frame, FrameBuilder, Wake};
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::play::Show;
use beryl_scene::table::{DESKTOP, Table, Window};
use beryl_scene::{Client, Config};

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

/// Every script of every recording in the pack, from each seat it is recorded for, finds on
/// the client's table everything its steps name. A step the table cannot take makes the step
/// after it place the whole table, so that its cards jump instead of moving. The client keeps
/// its table from script to script because the scripts number the abilities on the stack and
/// later steps name them by number, which a view alone does not say.
#[test]
fn every_recorded_script_finds_what_its_steps_name() {
    let names = recordings::names().unwrap_or_else(|why| panic!("{why}"));
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let failed: Vec<String> = std::thread::scope(|scope| {
        let workers: Vec<_> = names
            .chunks(names.len().div_ceil(threads).max(1))
            .map(|chunk| scope.spawn(move || chunk.iter().filter_map(unplaced).collect::<Vec<_>>()))
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a worker finishes"))
            .collect()
    });
    assert_eq!(
        failed,
        Vec::<String>::new(),
        "script steps the client's table could not take"
    );
}

/// How many steps of the recording `name`'s scripts the client's table could not take as
/// `seat` is sent them, as a line naming it, or none if it took them all.
fn unplaced((name, seat): &(String, Seat)) -> Option<String> {
    let side = if *seat == Seat::P0 {
        Side::You
    } else {
        Side::Opponent
    };
    let views = recording(name, side)
        .and_then(|recording| recording.views())
        .unwrap_or_else(|why| panic!("{why}"));
    let mut show = Show::new(Table::new(DESKTOP, Window::default()));
    for update in views {
        let timings = timings(&update);
        show.push(Box::new(update), timings);
    }
    (show.unplaced() > 0).then(|| format!("{name} from {seat:?}: {}", show.unplaced()))
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
            let placement = show.table().placement(&update.view, &[]);
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
fn pay_default_plays_to_its_end_from_both_seats_by_pressing_its_buttons() {
    for seat in SEATS {
        let mut page = Page::fixture("pay-default", seat, Layout::Desktop);
        let mut pressed = Vec::new();
        // How many views had been given when a button was last pressed: each view's prompt is
        // answered once, as soon as the table rests in it.
        let mut answered = 0;
        loop {
            page.check();
            if page.ended() {
                break;
            }
            let (given, _, moving) = page.position();
            if !moving
                && given > answered
                && let Some(button) = first_button(&page.tree())
            {
                page.press(&button);
                pressed.push(button);
                answered = given;
                continue;
            }
            page.step(TICK);
        }
        let names: Vec<&str> = pressed
            .iter()
            .map(|locator| locator.split('"').nth(1).unwrap_or_default())
            .collect();
        // The viewer answers a priority stop, where the recording's stand-in cast a spell, and
        // then pays its cost automatically; the recording ends at the next priority stop. The
        // opponent decides nothing in it.
        let expected: &[&str] = match seat {
            Side::You => &["Next", "Auto Pay"],
            Side::Opponent => &[],
        };
        assert_eq!(names, expected, "{seat:?}: the buttons pressed");
    }
}

#[test]
fn pay_default_shows_each_prompt_in_the_prompt_bar() {
    let mut page = Page::fixture("pay-default", Side::You, Layout::Desktop);
    for (moment, lines) in [
        ("prompt:1", &["button \"Next\""][..]),
        (
            "prompt:2",
            &["text \"Pay {1}{W}.\"", "button \"Auto Pay\""][..],
        ),
        (
            "prompt:3",
            &["button \"Resolve\"", "button \"To Main 2\""][..],
        ),
    ] {
        page.seek(&moment.parse().expect("a moment"))
            .expect("the recording has the moment");
        let tree = page.tree();
        let indent = |line: &str| line.len() - line.trim_start().len();
        let mut lines_from_bar = tree
            .lines()
            .skip_while(|line| !line.trim().starts_with("region \"prompt\""));
        let depth = lines_from_bar.next().map_or(0, indent);
        // The lines indented under the bar's: the floats after it stand beside it.
        let bar: Vec<&str> = lines_from_bar
            .take_while(|line| indent(line) > depth)
            .map(str::trim)
            .collect();
        assert_eq!(bar.len(), lines.len(), "{moment}: the bar holds\n{tree}");
        for (line, start) in bar.iter().zip(lines) {
            assert!(line.starts_with(start), "{moment}: {line:?} is {start}");
            assert_eq!(
                line.split_whitespace().any(|word| word == "interactive"),
                line.starts_with("button "),
                "{moment}: each button, and only a button, can be pressed: {line:?}"
            );
        }
    }
}

#[test]
fn a_repeated_pay_default_view_lays_out_and_shapes_nothing() {
    for seat in SEATS {
        let views = recording("pay-default", seat)
            .and_then(|recording| recording.views())
            .unwrap_or_else(|why| panic!("{why}"));
        let mut client = Client::new(Config::LARGEST);
        client.handle(Input::Surface {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        });
        let mut now = 0;
        for (index, update) in views.iter().enumerate() {
            client.handle(Input::Update(Box::new(update.clone())));
            client.frame(now);
            now += TICK.0;
            client.handle(Input::Update(Box::new(update.clone())));
            let work = client.frame(now).work;
            assert_eq!(
                (work.layout_nodes, work.text_runs),
                (0, 0),
                "{seat:?} view {}: the same view again",
                index + 1
            );
            now += TICK.0;
            let work = client.frame(now).work;
            assert_eq!(
                (work.layout_nodes, work.text_runs),
                (0, 0),
                "{seat:?} view {}: a frame with no input",
                index + 1
            );
        }
    }
}

/// The locator of the first button that can be pressed in a tree printout, as its role, name
/// and id.
fn first_button(tree: &str) -> Option<String> {
    let line = tree.lines().map(str::trim).find(|line| {
        line.starts_with("button \"") && line.split_whitespace().any(|word| word == "interactive")
    })?;
    let id = line.find(" #")? + 2;
    let end = line[id..].find(' ').map_or(line.len(), |at| id + at);
    Some(line[..end].to_owned())
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
fn timings(update: &beryl_protocol::GameUpdate) -> Vec<beryl_protocol::script::StepTiming> {
    update
        .view
        .script
        .as_ref()
        .map(|script| script.timing.clone())
        .unwrap_or_default()
}

/// Every line of the tree printout, pasted back as a locator, finds the element it was printed
/// for, at every check time of `verbs-zones` and `pay-default` from both seats (each input, and
/// each start, middle and end of a segment or step): cards that share a name, cards in flight
/// and the prompt bar's elements alike.
#[test]
fn every_tree_line_locates_its_own_element_at_every_check_time() {
    for name in ["verbs-zones", "pay-default"] {
        for seat in SEATS {
            let mut page = Page::fixture(name, seat, Layout::Desktop);
            for t in page.check_times() {
                page.seek(&Moment::At(Duration(t)))
                    .expect("a time is a moment");
                let clock = page.clock();
                let frame = page.frame();
                for index in 0..frame.resolved.len() as u32 {
                    let printed = line(frame, index);
                    assert_eq!(
                        locate(frame, &printed),
                        Ok(index),
                        "{name} {seat:?} {clock}: '{printed}' does not locate its own element"
                    );
                }
            }
        }
    }
}
