//! Drive scripts and load threads: a script played twice over the same recording on the same
//! ticks gives the same frames, its pointer reaches the client as a person's would, a locator
//! that finds nothing stops the script with the line that named it, and each load thread is in
//! the class it was given.

use app_client::Input;
use app_client::{Client, Config};
use beryl_3d::fixture::Fixture;
use beryl_3d::load;
use beryl_host::{Class, Draw, Drive, Feed, FrameLoop, Shown, Ticked, Ticks};
use beryl_session::protocol::Side;
use kit_frame::frame::{Frame, Wake};
use kit_probe::agent::{TreeOptions, tree};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

/// A 60 Hz display's interval, in nanoseconds.
const INTERVAL: u64 = 16_666_667;

/// Ticks a test gives by hand.
struct TestTicks;

impl Ticks for TestTicks {
    fn set_running(&mut self, _running: bool) {}
}

/// A recording replayed on the frame loop's clock, as the app's `--fixture` replays it.
struct Replay(Fixture);

impl Feed for Replay {
    fn before(&mut self, client: &mut Client, clock: u64) {
        for update in self.0.due(clock) {
            client.handle(Input::Update(Box::new(update)));
        }
    }

    fn after(&mut self, client: &mut Client, _ticked: &Ticked) {
        while let Some(command) = client.next_command() {
            let (reply, views) = self.0.answer(&command).expect("the recording answers");
            client.handle(Input::Reply(reply));
            for update in views {
                client.handle(Input::Update(Box::new(update)));
            }
        }
        self.0.settled(client);
    }

    fn wake(&self) -> Wake {
        self.0.wake()
    }
}

/// Each frame drawn, as the tree `just b3d tree` prints.
#[derive(Default)]
struct Trees(Vec<String>);

impl Draw for Trees {
    fn draw(&mut self, frame: &Frame) -> Option<Shown> {
        self.0
            .push(tree(frame, &TreeOptions::default()).expect("a whole tree prints"));
        Some(Shown::default())
    }
}

/// Plays `script` over `verbs-zones` on 60 Hz ticks for `ticks` ticks, or until the script
/// ends, and returns every frame drawn and how the script ended.
fn play(script: &str, ticks: u64) -> (Vec<String>, Option<Result<(), String>>) {
    let drive = Drive::parse("test", script).expect("the script reads");
    let fixture = Fixture::new("verbs-zones", Side::You).expect("the recording reads");
    let mut frames = FrameLoop::new(
        Client::new(crate::test_data::data(), Config::LARGEST),
        TestTicks,
    )
    .with_feed(Box::new(Replay(fixture)))
    .driven(Some(drive));
    frames.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 1.0,
        safe_area: Insets::default(),
    }));
    let mut trees = Trees::default();
    for tick in 1..=ticks {
        if frames.at_end().is_some() {
            break;
        }
        frames.tick(tick * INTERVAL, &mut trees);
    }
    (trees.0, frames.at_end())
}

const SWEEP: &str = "\
# The pointer crosses the hand, then rests over the battlefield.
at 1s move 440,820
at 1100ms move 620,820
at 1200ms move 800,820
step 150ms
at 1400ms move 980,820
at 2s move 800,300
";

#[test]
fn a_script_played_twice_draws_the_same_frames() {
    let (first, first_end) = play(SWEEP, 300);
    let (second, second_end) = play(SWEEP, 300);
    assert_eq!(first_end, Some(Ok(())), "the script did not end by itself");
    assert_eq!(second_end, first_end);
    assert_eq!(
        first.len(),
        second.len(),
        "the runs drew different numbers of frames"
    );
    assert!(first == second, "the two runs drew different frames");
    // The pointer reached the client as a person's: a card in the hand was hovered.
    assert!(
        first.iter().any(|tree| tree
            .lines()
            .any(|line| line.contains("hand.yours") && line.contains("hovered"))),
        "no frame hovered a card in the hand"
    );
}

#[test]
fn a_locator_that_finds_nothing_stops_the_script_at_its_line() {
    let script = "at 500ms move 600,820\n\nat 1s hover card \"Black Lotus\"\nat 2s move 1,1\n";
    let (_, end) = play(script, 300);
    let error = end
        .expect("the script stopped")
        .expect_err("the script failed");
    assert!(error.starts_with("test:3: "), "{error}");
}

#[test]
fn a_line_that_is_not_an_action_is_refused_with_its_number() {
    let error = Drive::parse("test", "at 1s move 1,1\nat 2s jump\n").expect_err("refused");
    assert!(error.starts_with("test:2: "), "{error}");
}

#[test]
fn load_threads_are_in_the_class_they_were_given() {
    assert_eq!(load::parse("16"), Ok((16, Class::Default)));
    assert_eq!(load::parse("4:utility"), Ok((4, Class::Utility)));
    assert!(load::parse("4:urgent").is_err());
    for class in [Class::Utility, Class::Default, Class::Interactive] {
        let load = load::start(2, class);
        let expected =
            cfg!(any(target_vendor = "apple", windows, target_os = "android")).then_some(class);
        assert_eq!(load.classes, [expected, expected], "{class:?}");
    }
}
