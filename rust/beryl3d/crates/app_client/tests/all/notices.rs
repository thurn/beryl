//! The verbs `announce` and `dwell` (docs/beryl-3d.md, "Verbs" and "Floats"): the turn banner
//! stands centred in the window for its step and goes on standing while the next beat plays
//! under it, a scry's pill stands above its player's life total, two notices at once neither
//! overlap nor share a name, a notice lays nothing out once shown, `Show::passed` is when the
//! banner ends, and a spell left to be read is drawn focused for its step.

use app_client::Input;
use app_client::driver::{Mode, Page, Recording};
use app_client::{Client, Config};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{ObjectId, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{Act, Notice, Subject, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_play::play::record::Planted;
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits, Wake};
use kit_probe::agent::checks::{duplicates, overlap};
use kit_probe::agent::{Context, Rule};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

use crate::common::{MS, empty_frame, frame, show};

const WIDTH: f32 = 1440.0;
const HEIGHT: f32 = 900.0;

fn turn(active: Side) -> Act {
    Act::Announce {
        notice: Notice::Turn { active },
    }
}

fn scry(player: Side) -> Act {
    Act::Announce {
        notice: Notice::Scry {
            player,
            top: 1,
            bottom: 1,
        },
    }
}

/// The sample table with a reset script, and the same table after a script of one beat for
/// each of `beats`, timed as the session times them, where `change` makes the view the script
/// leads to.
fn scripted(beats: Vec<Vec<Act>>, change: impl FnOnce(&mut GameUpdate)) -> [GameUpdate; 2] {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let mut after = start.clone();
    change(&mut after);
    let mut script = ScriptBuilder::default();
    for beat in beats {
        script.beat(beat);
    }
    let mut script = script.build();
    script.timing = time_script(&script, &Played::of(&start.view));
    after.view.script = Some(script);
    [start, after]
}

/// A client at 1440 by 900 that shows `updates[0]` at the frame at 0 and is then given
/// `updates[1]`, whose script starts at the frame at 1 ms; and when each step of that script
/// begins and ends, in nanoseconds since the client started.
fn client(updates: [GameUpdate; 2]) -> (Client, Vec<(u64, u64)>) {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 256,
            draws: 1024,
            name_bytes: 4096,
        },
    );
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: WIDTH,
            height: HEIGHT,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.record();
    let [start, after] = updates;
    client.handle(Input::Update(Box::new(start)));
    client.frame(0);
    client.handle(Input::Update(Box::new(after)));
    client.frame(MS);
    let record = &client.history()[1];
    let begun = record.start.expect("the script has started");
    let steps = (record.steps.iter())
        .map(|step| (begun + step.from, begun + step.to))
        .collect();
    (client, steps)
}

/// The box of the text element showing `words` in the frame, if there is one: its least and
/// greatest corner, in logical points.
fn text_box(frame: &Frame, words: &str) -> Option<(Vec2, Vec2)> {
    let element = (frame.resolved.iter())
        .find(|element| element.role == Role::Text && frame.name(element.name) == words)?;
    let corners = element.corners;
    let min = corners.into_iter().reduce(Vec2::min).expect("four corners");
    let max = corners.into_iter().reduce(Vec2::max).expect("four corners");
    Some((min, max))
}

/// The box of the life total of the player `name`, which is named for them and its number.
fn life_box(frame: &Frame, name: &str) -> (Vec2, Vec2) {
    let element = (frame.resolved.iter())
        .find(|element| {
            element.role == Role::Player
                && frame
                    .name(element.name)
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with(' '))
        })
        .expect("a life total for the player");
    let corners = element.corners;
    let min = corners.into_iter().reduce(Vec2::min).expect("four corners");
    let max = corners.into_iter().reduce(Vec2::max).expect("four corners");
    (min, max)
}

#[test]
fn a_fault_can_be_planted_while_a_banner_outlasts_its_script() {
    let [start, after] = scripted(vec![vec![turn(Side::You)]], |_| {});
    let mut messages = vec![
        FixtureMessage::game_update(&start),
        FixtureMessage::game_update(&after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let size = Size {
        width: WIDTH,
        height: HEIGHT,
    };
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        size,
    )
    .unwrap();
    let banner = page.steps().pop().expect("the turn banner");
    page.plant(Planted::Crowded, banner.middle).unwrap();
    assert!(
        page.check_all(Mode::AtCheckTimes)
            .faults
            .iter()
            .any(|fault| fault.rule == Rule::Crowded)
    );
}

#[test]
fn the_turn_banner_stands_centred_for_its_step_and_then_goes() {
    let (mut client, steps) = client(scripted(vec![vec![turn(Side::You)]], |_| {}));
    let (from, to) = steps[0];
    assert!(text_box(client.frame(from - 1), "Your Turn").is_none());
    for now in [from, from + (to - from) / 2, to - 1] {
        let frame = client.frame(now);
        let (min, max) = text_box(frame, "Your Turn").expect("the banner");
        let middle = (min + max) / 2.0;
        // In the last fifth of its run the banner sinks 30 points toward the player whose turn it
        // is, so only its horizontal centre holds there.
        let sunk = if now == to - 1 { 30.0 } else { 0.0 };
        assert!(
            (middle - Vec2::new(WIDTH / 2.0, HEIGHT / 2.0 + sunk))
                .abs()
                .max_element()
                <= 1.0,
            "the banner is centred: {middle} at {now}"
        );
        assert!(text_box(frame, "Opponent's Turn").is_none());
    }
    let frame = client.frame(from + (to - from) / 2);
    let element = (frame.resolved.iter())
        .find(|element| frame.name(element.name) == "Your Turn")
        .expect("the banner");
    assert!(
        (element.opacity - 1.0).abs() < f32::EPSILON,
        "it stands whole mid-run"
    );
    assert!(
        text_box(client.frame(to), "Your Turn").is_none(),
        "the banner has gone at its end"
    );
}

#[test]
fn the_banner_stands_while_the_next_beat_draws_a_card() {
    let (updates, top) = banner_then_draw();
    let (mut client, steps) = client(updates);
    let (banner_from, banner_to) = steps[0];
    let (fly_from, fly_to) = steps[1];
    assert!(
        fly_from < banner_to,
        "the draw begins while the banner stands: {fly_from} before {banner_to}"
    );
    let now = fly_from + (fly_to.min(banner_to) - fly_from) / 2;
    assert!(now > banner_from);
    let frame = client.frame(now);
    assert!(
        text_box(frame, "Opponent's Turn").is_some(),
        "the banner stands"
    );
    let card = (frame.resolved.iter())
        .find(|element| element.role == Role::Card && element.id.0 == u32::from(top.0))
        .expect("the drawn card");
    assert!(card.state.has(StateBits::MOVING), "the card flies under it");
}

/// The opponent's turn banner, then a beat in which you draw the top card of your library, with
/// the slot of the card drawn.
fn banner_then_draw() -> ([GameUpdate; 2], ObjectId) {
    let sample = ViewBuilder::sample().update();
    let top = *sample.view.players.you.library.last().expect("a library");
    let mut card = (sample.view.objects.iter())
        .find(|object| object.slot == top)
        .expect("the top card")
        .clone();
    card.zone = ZoneName::Hand;
    let drawn = card.clone();
    let draw = Act::Fly {
        from: ZoneName::Library,
        why: Why::Draw,
        object: card,
        at: None,
        item: None,
        origin: None,
    };
    let updates = scripted(vec![vec![turn(Side::Opponent)], vec![draw]], |after| {
        let view = &mut after.view;
        view.players.you.library.retain(|&slot| slot != top);
        if let Some(object) = view.objects.iter_mut().find(|o| o.slot == top) {
            *object = drawn;
        }
    });
    (updates, top)
}

/// The recorded reply to a view is due a fixed time after the table rests, so the table must
/// rest at the same time however its frames fall. A banner that outlasted its script kept the
/// table awake, but the rest was taken from the script's end, so a frame that first found the
/// banner gone gave the reply at once: checking every tick gave replies at other times than
/// checking at the check times, and the two disagreed about the steps that followed.
#[test]
fn the_table_rests_as_a_banner_that_outlasts_its_script_ends_however_the_frames_fall() {
    let (_, steps) = client(banner_then_draw().0);
    let (_, banner_to) = steps[0];
    let rest = steps.iter().map(|&(_, to)| to).max().expect("two steps");
    assert!(
        rest == banner_to,
        "the banner ends last: {banner_to} of {rest}"
    );
    for late in [0, 1, 17 * MS, 500 * MS] {
        let (mut client, _) = client(banner_then_draw().0);
        client.frame(banner_to + late);
        assert_eq!(client.wake(), Wake::Never, "drawn {late} ns late");
        assert_eq!(client.rested(), banner_to, "drawn {late} ns late");
    }
}

#[test]
fn a_scry_pill_stands_above_its_players_life_total() {
    for (side, name) in [(Side::You, "You"), (Side::Opponent, "Opponent")] {
        let (mut client, steps) = client(scripted(vec![vec![scry(side)]], |_| {}));
        let (from, to) = steps[0];
        let frame = client.frame(from + (to - from) / 2);
        let (pill_min, pill_max) =
            text_box(frame, "Scry: 1 top, 1 bottom.").expect("the scry pill");
        let (life_min, life_max) = life_box(frame, name);
        assert!(
            pill_max.y <= life_min.y,
            "{name}'s pill stands above their total"
        );
        let pill_middle = f32::midpoint(pill_min.x, pill_max.x);
        let life_middle = f32::midpoint(life_min.x, life_max.x);
        assert!(
            (pill_middle - life_middle).abs() <= 1.0,
            "{name}'s pill is centred over their total"
        );
    }
}

#[test]
fn two_notices_at_once_do_not_overlap() {
    let (mut client, steps) = client(scripted(
        vec![vec![turn(Side::You), scry(Side::Opponent)]],
        |_| {},
    ));
    let (from, to) = steps[0];
    let frame = client.frame(from + (to - from) / 2);
    let banner = text_box(frame, "Your Turn").expect("the banner");
    let pill = text_box(frame, "Scry: 1 top, 1 bottom.").expect("the pill");
    let apart = banner.1.x <= pill.0.x
        || pill.1.x <= banner.0.x
        || banner.1.y <= pill.0.y
        || pill.1.y <= banner.0.y;
    assert!(apart, "the banner {banner:?} and the pill {pill:?} overlap");
    let context = Context {
        size_class: crate::REGULAR,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(crate::REGULAR)
            .checks,
        size: Size {
            width: WIDTH,
            height: HEIGHT,
        },
        safe_area: Insets::default(),
        at: 0,
    };
    assert_eq!(overlap(frame, &context), Vec::new());
    assert_eq!(duplicates(frame, &context), Vec::new());
}

#[test]
fn a_notice_lays_nothing_out_once_shown() {
    let (mut client, steps) = client(scripted(vec![vec![turn(Side::You)]], |_| {}));
    let (from, to) = steps[0];
    let mut now = from;
    while now <= to {
        let work = client.frame(now).work;
        assert_eq!(work.layout_nodes, 0, "layout at {now}");
        assert_eq!(work.text_runs, 0, "text shaped at {now}");
        now += 16 * MS;
    }
}

#[test]
fn passed_is_when_the_banner_ends() {
    let [start, after] = scripted(vec![vec![turn(Side::You)]], |_| {});
    let timings = after.view.script.as_ref().expect("a script").timing.clone();
    let mut show = show();
    show.record();
    let mut scratch = empty_frame();
    show.push(Box::new(start), Vec::new());
    frame(&mut show, &mut scratch, 0);
    assert_eq!(show.passed(), 0, "no notice is held");
    show.push(Box::new(after), timings);
    frame(&mut show, &mut scratch, MS);
    let record = &show.history()[1];
    let begun = record.start.expect("the script has started");
    let end = begun + record.steps[0].to;
    assert_eq!(show.passed(), end);
    frame(&mut show, &mut scratch, end);
    assert_eq!(show.passed(), 0, "the banner has gone");
}

#[test]
fn a_spell_left_to_be_read_is_focused_for_its_step() {
    let sample = ViewBuilder::sample().update();
    let spell = (sample.view.stack.iter())
        .find_map(|item| match item {
            StackItem::Spell { slot, .. } => Some(*slot),
            StackItem::Ability { .. } => None,
        })
        .expect("a spell on the stack");
    let read = Act::Dwell {
        subject: Subject::Object { slot: spell },
    };
    let (mut client, steps) = client(scripted(vec![vec![read]], |_| {}));
    let (from, to) = steps[0];
    let focused = |frame: &Frame| {
        (frame.resolved.iter())
            .find(|element| element.role == Role::Card && element.id.0 == u32::from(spell.0))
            .expect("the spell")
            .state
            .has(StateBits::FOCUSED)
    };
    assert!(!focused(client.frame(from - 1)), "not before its step");
    assert!(focused(client.frame(from)), "from its step's start");
    assert!(focused(client.frame(to - 1)), "until its end");
    assert!(!focused(client.frame(to)), "and no longer");
}

/// Each kind of notice fades as the web stage's does (`.stage-notice` in
/// `ui/src/stage/stage.css`): a fifth of the way through its run, the turn banner is still
/// coming up, over its first 28%, while a scry's pill, which comes up over its first 5%, stands
/// whole.
#[test]
fn each_kind_of_notice_fades_as_the_webs_does() {
    let opacity_at = |act: Act, words: &str, share: u64| {
        let (mut client, steps) = client(scripted(vec![vec![act]], |_| {}));
        let (from, to) = steps[0];
        let frame = client.frame(from + (to - from) * share / 100);
        (frame.resolved.iter())
            .find(|element| frame.name(element.name) == words)
            .expect("the notice is shown")
            .opacity
    };
    let banner = opacity_at(turn(Side::You), "Your Turn", 20);
    assert!(
        (0.6..0.8).contains(&banner),
        "the banner is about 0.2 / 0.28 of the way up: {banner}"
    );
    let pill = opacity_at(scry(Side::You), "Scry: 1 top, 1 bottom.", 20);
    assert!(
        (pill - 1.0).abs() < f32::EPSILON,
        "the pill stands whole: {pill}"
    );
}
