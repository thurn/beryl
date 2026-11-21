//! The `Page` test driver, on recordings built by hand.

use app_client::Input;
use app_client::command::{Reply, SessionCommand};
use app_client::driver::{Duration, Name, Page, Recording, Responses};
use beryl_protocol::ipc::{ChooseRequest, FixtureMessage};
use beryl_protocol::prompt::{ButtonId, Click, ClickReply, ClickRequest};
use beryl_protocol::protocol::{Side, Status, Why, ZoneName};
use beryl_protocol::script::{Act, Subject};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, timing};
use beryl_protocol::{CandidateIndex, DecisionId, SessionId};
use glam::Vec2;
use kit_frame::frame::StateBits;
use kit_probe::agent::Layout;
use kit_ui::input::{KeyCode, PointerPhase, Size};

fn ms(milliseconds: u64) -> Duration {
    Duration::from_millis(milliseconds)
}

/// The sample table, then the same table with one more card in the viewer's hand, then with
/// another: three views, each answered by a recorded reply.
fn three_views() -> Vec<ViewBuilder> {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let first = view.clone();
    view.card(ZoneName::Hand, Side::You, "Opt");
    let second = view.clone();
    view.card(ZoneName::Hand, Side::You, "Ponder");
    vec![first, second, view]
}

fn recording(views: &[ViewBuilder]) -> Recording {
    Recording(
        views
            .iter()
            .map(|view| FixtureMessage::game_update(&view.update()))
            .collect(),
    )
}

fn page(views: &[ViewBuilder]) -> Page {
    Page::new(
        crate::test_data::data(),
        recording(views).into(),
        Side::You,
        Layout::Desktop,
        Size {
            width: 1600.0,
            height: 900.0,
        },
    )
    .unwrap()
}

#[test]
fn a_recording_plays_on_its_own_clock() {
    let mut page = page(&three_views());
    let moments = page.moments();
    assert_eq!(
        moments,
        vec![
            (Name::Start, ms(0)),
            (Name::View(1), ms(0)),
            (Name::View(2), ms(400)),
            (Name::View(3), ms(800)),
            (Name::End, ms(800)),
        ]
    );
    // Stepping the clock gives each reply when it is due, as the moments say.
    page.step(ms(399));
    assert!(page.try_find("card \"Opt\"").is_err());
    page.step(ms(1));
    assert_eq!(page.find("card \"Opt\"").zone(), Some(ZoneName::Hand));
}

#[test]
fn seeking_goes_to_a_moment_and_back() {
    let mut page = page(&three_views());
    page.seek(&"view:3".parse().unwrap()).unwrap();
    assert_eq!(page.now(), ms(800).0);
    page.find("card \"Ponder\"");
    page.seek(&"view:2+50ms".parse().unwrap()).unwrap();
    assert_eq!(page.now(), ms(450).0);
    page.find("card \"Opt\"");
    assert!(page.try_find("card \"Ponder\"").is_err());

    let error = page
        .seek(&"prompt:1".parse().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("no moment 'prompt:1'"), "{error}");
    assert!(error.contains("view:3"), "{error}");
    // The views start the table over and play no steps.
    let error = page
        .seek(&"step:2.0.0".parse().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("no moment 'step:2.0.0'"), "{error}");
}

/// The sample table, then the viewer drawing Opt and then Ponder, in two beats of one script
/// timed 400 ms and 300 ms.
fn two_draws() -> Vec<ViewBuilder> {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let first = view.clone();
    let opt = view.card(ZoneName::Hand, Side::You, "Opt").clone();
    let ponder = view.card(ZoneName::Hand, Side::You, "Ponder").clone();
    let draw = |object| Act::Fly {
        from: ZoneName::Library,
        why: Why::Draw,
        object,
        at: None,
        item: None,
        origin: None,
    };
    let mut script = ScriptBuilder::default()
        .beat([draw(opt)])
        .beat([draw(ponder)])
        .build();
    script.timing = vec![timing(400.0), timing(300.0)];
    view.script(script);
    vec![first, view]
}

#[test]
fn step_moments_come_from_the_script_clock() {
    let mut page = page(&two_draws());
    let moments = page.moments();
    // The second view arrives 400 ms after the first rests and its script starts then. Each
    // step's moment is the middle of the span the script clock gave it.
    let first = moments
        .iter()
        .find(|(name, _)| *name == Name::Step(2, 0, 0))
        .map(|&(_, time)| time);
    let second = moments
        .iter()
        .find(|(name, _)| *name == Name::Step(2, 1, 0))
        .map(|&(_, time)| time);
    assert_eq!(first, Some(ms(600)), "{moments:?}");
    // The second beat starts when the first ends, at 800 ms, and lasts 300 ms.
    assert_eq!(second, Some(ms(950)), "{moments:?}");
    // The steps follow their view, and the end follows them.
    let names: Vec<Name> = moments.iter().map(|&(name, _)| name).collect();
    assert_eq!(
        names,
        vec![
            Name::Start,
            Name::View(1),
            Name::View(2),
            Name::Step(2, 0, 0),
            Name::Step(2, 1, 0),
            Name::End,
        ]
    );

    page.seek(&"step:2.1.0".parse().unwrap()).unwrap();
    assert_eq!(page.now(), ms(950).0);
    let ponder = page.find("card \"Ponder\"");
    assert!(ponder.states().has(StateBits::MOVING), "{}", page.tree());
    // The cards already in the hand slide to make room; the stack stays still.
    assert!(
        !page
            .find("card \"Lightning Bolt\"")
            .states()
            .has(StateBits::MOVING)
    );
    // Seeking back starts the recording over and lands in the first step.
    page.seek(&"step:2.0.0".parse().unwrap()).unwrap();
    assert_eq!(page.now(), ms(600).0);
    assert!(page.find("card \"Opt\"").states().has(StateBits::MOVING));
    page.still();
    assert!(!page.find("card \"Ponder\"").states().has(StateBits::MOVING));
}

#[test]
fn views_the_game_plays_on_through_arrive_together() {
    let views = three_views();
    let mut recording = recording(&views);
    let mut update = views[1].update();
    update.view.status = Status::Playing;
    recording.0[1] = FixtureMessage::game_update(&update);
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut page = Page::new(
        crate::test_data::data(),
        recording.into(),
        Side::You,
        Layout::Desktop,
        size,
    )
    .unwrap();
    let moments = page.moments();
    assert!(moments.contains(&(Name::View(2), ms(400))), "{moments:?}");
    assert!(moments.contains(&(Name::View(3), ms(400))), "{moments:?}");
    assert_eq!(moments.last(), Some(&(Name::End, ms(400))));
}

#[test]
fn a_script_of_actions_makes_the_same_inputs_as_the_methods() {
    let views = three_views();
    let mut by_methods = page(&views);
    by_methods.press("card \"Counterspell\"");
    by_methods.hover("card \"Memory Lapse\"");
    by_methods.drag("card \"Counterspell\"", "card \"Lightning Bolt\"");
    by_methods.key(KeyCode::Enter);
    by_methods.move_to(Vec2::new(400.0, 300.0));

    let mut by_script = page(&views);
    by_script.run(
        "press card \"Counterspell\"\n\
         hover card \"Memory Lapse\"\n\
         # A comment, and a blank line, are skipped.\n\
         \n\
         drag card \"Counterspell\" to card \"Lightning Bolt\"\n\
         key Enter\n\
         move 400,300",
    );
    assert_eq!(by_script.inputs(), by_methods.inputs());
    assert_eq!(by_script.now(), by_methods.now());
    // The drag stepped the clock 16 ms after each of its eight moves.
    assert_eq!(by_methods.now(), ms(8 * 16).0);
}

#[test]
fn a_press_lands_inside_the_uncovered_part_of_its_element() {
    let mut page = page(&three_views());
    let element = page.find("card \"Counterspell\"");
    let frame = page.frame().clone();
    let rect = kit_scene::hit::uncovered(&frame, element.index).unwrap();
    page.press("card \"Counterspell\"");
    let presses: Vec<Vec2> = page
        .inputs()
        .iter()
        .filter_map(|input| match input {
            Input::Pointer(pointer) if pointer.phase == PointerPhase::Down => {
                Some(pointer.position)
            }
            _ => None,
        })
        .collect();
    let [point] = presses.as_slice() else {
        panic!("one press: {presses:?}");
    };
    assert!(
        point.x > rect.x
            && point.x < rect.x + rect.width
            && point.y > rect.y
            && point.y < rect.y + rect.height,
        "{point} outside {rect:?}"
    );
    assert_eq!(kit_scene::hit::hit(&frame, *point), Some(element.index));
}

#[test]
fn a_failed_find_names_the_nearest_elements_and_the_clock() {
    let mut page = page(&three_views());
    let error = page
        .try_find("card \"Counterspel\"")
        .unwrap_err()
        .to_string();
    assert!(error.contains("card \"Counterspell\" #"), "{error}");
    assert!(error.ends_with("t=0ms view 1 of 3, at rest"), "{error}");
}

fn decision() -> DecisionId {
    DecisionId {
        session: SessionId(1),
        seq: 7,
    }
}

#[test]
fn commands_are_answered_from_the_recorded_responses() {
    let request = |index| ChooseRequest {
        decision: decision(),
        index: CandidateIndex(index),
        continuation: None,
    };
    let recording = Recording(vec![
        FixtureMessage::asked("choose", &request(1), &()),
        FixtureMessage::response("click", &ClickReply::Answered),
    ]);
    let mut responses = Responses::new(&recording);
    let answer = responses
        .answer(&SessionCommand::Choose(request(1)))
        .unwrap();
    assert_eq!(answer.reply, Reply::Done(Ok(())));
    assert!(answer.plays_on);
    // The last response for a command stays to answer it again, but only for its request.
    let error = responses
        .answer(&SessionCommand::Choose(request(2)))
        .unwrap_err();
    assert!(error.contains("recorded for"), "{error}");
    let error = responses
        .answer(&SessionCommand::PassOn(decision()))
        .unwrap_err();
    assert!(error.contains("`pass_on`"), "{error}");
    let error = responses
        .answer(&SessionCommand::Undo(decision()))
        .unwrap_err();
    assert!(error.contains("`undo`"), "{error}");
}

#[test]
fn a_button_press_takes_the_recorded_answer_of_a_recording_without_clicks() {
    let recording = Recording(vec![FixtureMessage::response("choose", &())]);
    let press = |button| {
        SessionCommand::Click(ClickRequest {
            decision: decision(),
            revision: 0,
            click: Click::Button { button },
        })
    };
    let mut responses = Responses::new(&recording);
    let answer = responses.answer(&press(ButtonId::Decline)).unwrap();
    assert_eq!(answer.reply, Reply::Click(Ok(ClickReply::Answered)));
    assert!(answer.plays_on);
    // A click on a card is not an answer the recording can stand for.
    let on_card = SessionCommand::Click(ClickRequest {
        decision: decision(),
        revision: 0,
        click: Click::On {
            on: Subject::Object {
                slot: beryl_protocol::protocol::ObjectId(3),
            },
        },
    });
    let error = responses.answer(&on_card).unwrap_err();
    assert!(error.contains("`click`"), "{error}");
    // A recording that holds clicks answers a press only from them.
    let recording = Recording(vec![
        FixtureMessage::response("choose", &()),
        FixtureMessage::asked(
            "click",
            &ClickRequest {
                decision: decision(),
                revision: 0,
                click: Click::Button {
                    button: ButtonId::Auto,
                },
            },
            &ClickReply::Answered,
        ),
    ]);
    let mut responses = Responses::new(&recording);
    let error = responses.answer(&press(ButtonId::Decline)).unwrap_err();
    assert!(error.contains("recorded for"), "{error}");
}
