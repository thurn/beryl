//! Recorded fixtures played from end to end through the client, with the timings the session
//! sent: `verbs-zones`, whose scripts move cards between every zone, `pay-default`, whose
//! prompts are answered by pressing the flat layer's buttons, and the stress boards.

use kit_ui::input::Input as DeviceInput;
use std::time::Instant;

use app_client::Input;
use app_client::driver::{Duration, Moment, Name, Page, TICK};
use app_client::{Client, Config};
use beryl_inspect::pages::{Recorded, check_recording, recording};
use beryl_inspect::{recordings, runner, stress};
use beryl_protocol::Seat;
use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::script::Verb;
use game_play::play::Show;
use game_table::table::Place;
use game_table::table::{Rest, Table, Window};
use kit_base::data::profiles::SizeClass;
use kit_base::pose::Pose;
use kit_frame::frame::{Frame, FrameBuilder, Wake};
use kit_probe::agent::locate;
use kit_probe::agent::tree::line;
use kit_ui::input::{Insets, Size, Tier};

const SEATS: [Side; 2] = [Side::You, Side::Opponent];

/// A millisecond, in nanoseconds.
const MS: u64 = 1_000_000;

/// The verbs that have motion of their own; every other verb is drawn as an instant change.
const BUILT: [Verb; 7] = [
    Verb::Fly,
    Verb::Appear,
    Verb::Vanish,
    Verb::Emerge,
    Verb::Strike,
    Verb::Count,
    Verb::Set,
];

#[test]
fn verbs_zones_plays_to_its_end_from_both_seats() {
    for seat in SEATS {
        let mut page = Page::fixture("verbs-zones", seat, SizeClass::Regular);
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

/// Each sound a frame lists comes within one tick of the start or the end of a step: the step
/// that asks for it. A strike's hit sounds part of the way through its step.
#[test]
fn verbs_zones_sounds_as_its_steps_begin_and_end() {
    for seat in SEATS {
        let mut page = Page::fixture("verbs-zones", seat, SizeClass::Regular);
        let steps = page.steps();
        let sounds = page.sounds();
        assert!(!sounds.is_empty(), "{seat:?}: the recording has sounds");
        for (time, sound) in sounds {
            let near = |edge: u64| (edge..=edge + TICK.0).contains(&time);
            assert!(
                steps.iter().any(|step| near(step.from)
                    || near(step.to)
                    || (step.verb == Verb::Strike
                        && (step.from..=step.to + TICK.0).contains(&time))),
                "{seat:?}: sound {sound:?} at {time} ns is not within a tick of a step's start or end"
            );
        }
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
    let failed: Vec<_> = runner::each(&names, unplaced)
        .into_iter()
        .flatten()
        .collect();
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
    let mut show = Show::new(Table::new(crate::test_data::data(), Window::default()));
    for update in views {
        let timings = timings(&update);
        show.push(Box::new(update), timings);
    }
    (show.unplaced() > 0).then(|| format!("{name} from {seat:?}: {}", show.unplaced()))
}

/// Whether a card drawn at `pose` lies in the pile `rest` places it in, within the distance a
/// card may settle there without a segment (`settle` in `game_play::play::step`).
fn settled_in_pile(rest: &Rest, pose: Option<Pose>) -> bool {
    let in_pile = matches!(
        rest.place,
        Place::Card {
            zone: ZoneName::Library | ZoneName::Graveyard | ZoneName::Exile,
            ..
        }
    );
    in_pile
        && pose.is_some_and(|pose| {
            pose.rotation == rest.pose.rotation && pose.position.distance(rest.pose.position) <= 2.0
        })
}

#[test]
fn after_each_verbs_zones_script_every_object_rests_at_its_placement() {
    for seat in SEATS {
        let views = recording("verbs-zones", seat)
            .and_then(|recording| recording.views())
            .unwrap_or_else(|why| panic!("{why}"));
        let window = Window {
            size_class: None,
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        };
        let mut show = Show::new(Table::new(crate::test_data::data(), window));
        let mut frame = Frame::with_capacity(4096, 8192, 65_536);
        let mut now = 0;
        for (index, update) in views.into_iter().enumerate() {
            show.push(Box::new(update.clone()), timings(&update));
            show.frame(
                now,
                &mut FrameBuilder::new(
                    &mut frame,
                    Tier::Baseline,
                    crate::test_data::data().card.size(),
                ),
            );
            // The show lists each hand in its order of arrival, where the session's view lists
            // it by slot, so the view placed is the one the show ends on.
            let latest = show.latest().expect("the view was given");
            let placement = show.table().placement(&latest.view, &[]);
            // Far past the script's end, each moving object is where its track ends, once every
            // step is compiled, as the frames up to then would compile them.
            let ended = u64::MAX / 4;
            show.prepare(ended);
            // An object that leaves for a hidden zone ends at that zone's pile and then has no
            // placement; the check after the script covers it.
            for object in &update.view.objects {
                let Some(rest) = placement.get(object.slot) else {
                    continue;
                };
                let pose = show.pose(usize::from(object.slot.0), ended);
                // A card that a step only moved a fraction of a millimetre within its pile, as a
                // pile on the desktop stands so that its top card stays where the web draws it,
                // takes no segment and takes its place as the script ends.
                if settled_in_pile(rest, pose) {
                    continue;
                }
                assert_eq!(
                    pose,
                    Some(rest.pose),
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
                show.frame(
                    now,
                    &mut FrameBuilder::new(
                        &mut frame,
                        Tier::Baseline,
                        crate::test_data::data().card.size(),
                    ),
                );
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
        let mut page = Page::fixture("pay-default", seat, SizeClass::Regular);
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
    let mut page = Page::fixture("pay-default", Side::You, SizeClass::Regular);
    for (moment, lines) in [
        // The primary button stands over its caption: where passing leads.
        (
            "prompt:1",
            &[
                "region \"turn action\"",
                "button \"Next\"",
                "text \"To Combat\"",
            ][..],
        ),
        (
            "prompt:2",
            &[
                "text \"Pay {1}{W}.\"",
                "region \"turn action\"",
                "button \"Auto Pay\"",
            ][..],
        ),
        (
            "prompt:3",
            &[
                "region \"turn action\"",
                "button \"Resolve\"",
                "button \"To Main 2\"",
                "region #",
                "icon #",
            ][..],
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
        let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
            scale: 1.0,
            safe_area: Insets::default(),
        }));
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
    for name in stress::Board::ALL.map(stress::Board::name) {
        let mut page = Page::fixture(name, Side::You, SizeClass::Regular);
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
            let mut page = Page::fixture(name, seat, SizeClass::Regular);
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

/// The recording drawn at `at` after the clock ran there in steps of `step`, or straight there
/// when `step` is zero.
fn trigger_order_tree(step: u64, at: u64) -> String {
    let size = Size {
        width: 1440.0,
        height: 900.0,
    };
    let mut page =
        beryl_inspect::pages::page("choice-trigger-order", Side::You, SizeClass::Regular, size)
            .expect("the recording can be read");
    if step > 0 {
        while page.now() + step < at {
            page.step(Duration(step));
        }
    }
    page.step(Duration(at - page.now()));
    page.tree()
}

#[test]
fn a_moment_is_the_same_however_often_the_clock_stopped_before_it() {
    // A notice shown from 9.8 s outlasts the script that announced it by about 400 ms, longer
    // than the reply delay. The next view used to be given at the first frame after the notice
    // ended, so a clock stopped every 7 ms gave it up to 7 ms later than one run straight
    // there, and every card the next scripts moved was elsewhere at 14 s.
    let at = 14_000 * MS;
    let straight = trigger_order_tree(0, at);
    let stepped = trigger_order_tree(7 * MS + 333, at);
    assert_eq!(straight, stepped);
}

#[test]
fn a_trigger_that_resolves_in_its_own_script_rises_onto_the_stack() {
    // Snapcaster Mage's enters trigger goes on the stack and resolves within one script, so
    // neither the view before it nor the one after holds the ability. The table used to draw
    // nothing for it while it rose from its source and stood on the stack, though its track
    // moved it and its arrows pointed from it.
    let size = Size {
        width: 1440.0,
        height: 900.0,
    };
    let mut page = beryl_inspect::pages::page("verbs-piles", Side::You, SizeClass::Regular, size)
        .expect("the recording can be read");
    let steps = page.steps();
    let emerge = steps
        .iter()
        .find(|step| step.name == Name::Step(2, 3, 0))
        .expect("the trigger emerges");
    let vanish = steps
        .iter()
        .find(|step| step.name == Name::Step(2, 5, 0))
        .expect("the trigger resolves in the same script");
    assert_eq!(emerge.verb, Verb::Emerge);
    assert_eq!(vanish.verb, Verb::Vanish);
    assert_eq!(emerge.subject, vanish.subject);
    assert!(emerge.to < vanish.from, "the trigger rests on the stack");

    let ability = |line: &str| line.contains("ability \"Snapcaster Mage\"");
    page.seek(&Moment::At(Duration(emerge.middle))).unwrap();
    let rising = page.tree();
    assert!(
        rising
            .lines()
            .any(|line| ability(line) && line.ends_with("moving")),
        "{rising}"
    );

    let rest = u64::midpoint(emerge.to, vanish.from);
    page.seek(&Moment::At(Duration(rest))).unwrap();
    let resting = page.tree();
    assert!(
        resting
            .lines()
            .any(|line| ability(line) && line.contains(" stack ") && !line.ends_with("moving")),
        "{resting}"
    );
    page.seek(&Moment::At(Duration(vanish.to + MS))).unwrap();
    let resolved = page.tree();
    assert!(!resolved.lines().any(ability), "{resolved}");
}
