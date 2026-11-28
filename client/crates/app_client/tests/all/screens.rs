//! The start screen, driven through the `Page` test driver: choosing an opponent, starting a
//! game, conceding a game in progress first, and the screen's place at startup.

use std::sync::Arc;

use app_client::Input;
use app_client::command::{Reply, Request};
use app_client::driver::{Duration, OPEN, Page, Recording};
use app_client::{Client, Config};
use beryl_protocol::ipc::{
    CONCEDE, ConcedeRequest, FixtureMessage, LIST_AGENTS, START_GAME, TABLE_HOST,
    TABLE_LOBBY_COMMAND, TableHostReply, TableHostRequest, TableHostSettings, TableLink,
};
use beryl_protocol::protocol::{LossReason, OutcomeView, Side, Status};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use beryl_protocol::{AgentView, SessionId, StartRequest};
use kit_base::data::Data;
use kit_frame::frame::StateBits;
use kit_probe::agent::{self, TreeOptions};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size, Window};

const START: &str = "button \"Start\"";

/// Three agents, as a host offers them: the start screen plays against the first.
fn agents() -> Vec<AgentView> {
    ["random", "greedy", "expert"]
        .map(|name| AgentView {
            name: name.to_owned(),
            title: name.to_uppercase(),
            description: format!("The {name} agent"),
        })
        .to_vec()
}

/// A start request against `agent` with no seed and no fair deal, at the effort slider's
/// default of 30M engine steps a decision.
fn request(agent: &str) -> StartRequest {
    StartRequest {
        agent: agent.to_owned(),
        seed: None,
        fair: false,
        deck: beryl_protocol::default_deck(),
        work: Some(30_000_000),
    }
}

/// `messages` played at 1440 by 900, settled.
fn page(messages: Vec<FixtureMessage>) -> Page {
    let size = Size {
        width: 1440.0,
        height: 900.0,
    };
    page_at(messages, size)
}

fn page_at(messages: Vec<FixtureMessage>, size: Size) -> Page {
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        size,
    )
    .unwrap();
    page.still();
    page
}

/// How many answers to commands the client has been given so far.
fn replies(page: &Page) -> usize {
    // The start screen asks for the saved tables itself whenever it opens.
    (page.inputs().iter())
        .filter(
            |input| matches!(input, Input::Reply(reply) if !matches!(reply, Reply::TableSaved(_))),
        )
        .count()
}

/// The start screen with no game: the host's agents and the start answered for `agent` alone.
/// The agents arrive before any view, which puts the client on the start screen.
fn no_game(agent: &str) -> Page {
    page(vec![
        FixtureMessage::response(LIST_AGENTS, &agents()),
        FixtureMessage::asked(START_GAME, &request(agent), &()),
    ])
}

#[test]
fn pressing_start_plays_the_first_agent_at_the_effort_the_slider_sets() {
    let at_20m = StartRequest {
        work: Some(20_000_000),
        ..request("random")
    };
    let mut page = page(vec![
        FixtureMessage::response(LIST_AGENTS, &agents()),
        FixtureMessage::asked(START_GAME, &at_20m, &()),
    ]);
    assert!(
        page.find(START).states().has(StateBits::PRIMARY),
        "the tree marks Start as the button the screen expects"
    );
    assert!(
        page.try_find("toggle \"GREEDY\"").is_err(),
        "no opponent is offered to choose"
    );
    page.find("text \"30M\"");
    // A press in the slider's middle sets its level to 50, which is 20M.
    page.press("slider \"AI effort\"");
    page.still();
    page.find("text \"20M\"");
    page.press(START);
    page.still();
    // A start for any other agent or work would not match the recorded request and fail the
    // page.
    assert_eq!(replies(&page), 1, "the start was sent and answered");
    assert!(
        page.try_find(START).is_err(),
        "the game screen replaced the start screen"
    );
}

#[test]
fn choosing_the_shared_deck_sends_its_key_and_keeps_the_fair_deal() {
    let dandan = StartRequest {
        deck: "forgetful-fish".to_owned(),
        fair: true,
        ..request("random")
    };
    let mut page = page(vec![
        FixtureMessage::response(LIST_AGENTS, &agents()),
        FixtureMessage::asked(START_GAME, &dandan, &()),
    ]);
    assert!(
        page.find("toggle \"WU Control\"")
            .states()
            .has(StateBits::SELECTED)
    );
    page.find("text \"WU Control mirror\"");
    page.find("toggle \"Fair deal\"");
    page.press("toggle \"Fair deal\"");
    page.press("toggle \"Dandân\"");
    page.still();
    assert!(
        page.find("toggle \"Dandân\"")
            .states()
            .has(StateBits::SELECTED)
    );
    page.find("text \"Dandân · shared deck\"");
    assert!(
        page.try_find("text \"Dandân mirror\"").is_err(),
        "{}",
        page.tree()
    );
    page.find("toggle \"Fair deal\"");
    page.press(START);
    page.still();
    // A request without the shared deck key or fair deal would not match the recording.
    assert_eq!(replies(&page), 1, "the start was sent and answered");
}

#[test]
fn the_host_form_defaults_to_internet_and_can_send_a_local_request() {
    for link in [TableLink::Iroh, TableLink::Local] {
        let mut page = page(vec![
            FixtureMessage::response(LIST_AGENTS, &agents()),
            FixtureMessage::asked(
                TABLE_HOST,
                &TableHostRequest {
                    link,
                    settings: TableHostSettings {
                        name: "Host".into(),
                        starting_life: None,
                        starting_hand_size: None,
                    },
                },
                &TableHostReply {
                    invite: String::new(),
                },
            ),
            FixtureMessage::response(TABLE_LOBBY_COMMAND, &()),
        ]);
        page.press("button \"Host a table\"");
        page.type_text("field \"table name\"", "Host");
        page.check();
        let local = "toggle \"Local only\"";
        assert!(!page.find(local).states().has(StateBits::SELECTED));
        if link == TableLink::Local {
            page.press(local);
            assert!(page.find(local).states().has(StateBits::SELECTED));
        }
        page.press("button \"Host\"");
        page.still();
        assert!(
            page.inputs()
                .iter()
                .any(|input| matches!(input, Input::Reply(Reply::TableOpen(Ok(())))))
        );
    }
}

#[test]
fn with_a_game_in_progress_the_start_button_concedes_it_first() {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    // The opponent is deciding: a game goes on.
    let mut update = view.update();
    update.view.status = Status::Thinking;
    let mut page = page(vec![
        FixtureMessage::response(LIST_AGENTS, &agents()),
        FixtureMessage::Event {
            event: OPEN.to_owned(),
            payload: "start".into(),
        },
        FixtureMessage::game_update(&update),
        FixtureMessage::asked(
            CONCEDE,
            &ConcedeRequest {
                session: SessionId(1),
            },
            &(),
        ),
        FixtureMessage::asked(START_GAME, &request("random"), &()),
    ]);
    page.find("button \"Return to Game\"");
    page.press("button \"New Game\"");
    page.still();
    assert_eq!(replies(&page), 0, "the first press only asks to confirm");
    page.press("button \"Concede & Start\"");
    page.still();
    assert_eq!(
        replies(&page),
        2,
        "the concession and the start were both sent"
    );
}

#[test]
fn a_fair_start_keeps_the_start_screen_until_the_new_game_sends_a_view() {
    // The session judges candidate deals for seconds before a fair game starts. The game screen
    // showed the finished game's result all that time, as if the window had frozen.
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let mut over = view.update();
    over.view.status = Status::Over {
        outcome: OutcomeView::Win {
            winner: Side::Opponent,
            reason: LossReason::Concede,
            by: None,
        },
    };
    let fair = StartRequest {
        fair: true,
        ..request("random")
    };
    let mut page = page(vec![
        FixtureMessage::response(LIST_AGENTS, &agents()),
        FixtureMessage::game_update(&over),
        FixtureMessage::asked(START_GAME, &fair, &()),
    ]);
    page.press("button \"New Game\"");
    page.still();
    page.press("toggle \"Fair deal\"");
    page.press(START);
    page.still();
    assert_eq!(replies(&page), 1, "the start was sent and answered");
    let dealing = "button \"Dealing…\"";
    assert!(page.find(dealing).states().has(StateBits::DISABLED));
    assert!(
        page.try_find("region \"game over\"").is_err(),
        "{}",
        page.tree()
    );
    // Enter presses the start button, which sends nothing more while the deal is judged.
    page.key(kit_ui::input::KeyCode::Enter);
    page.still();
    assert_eq!(replies(&page), 1, "a second start was sent");
    let mut next = ViewBuilder::sample();
    next.script(ScriptBuilder::reset().build());
    let mut update = next.update();
    update.session = SessionId(over.session.0 + 1);
    page.give_input(Input::Update(Box::new(update)));
    page.still();
    assert!(
        page.try_find("region \"start\"").is_err(),
        "{}",
        page.tree()
    );
    assert!(page.try_find("region \"game over\"").is_err());
}

#[test]
fn a_seed_that_is_not_a_number_disables_start() {
    let mut page = no_game("random");
    page.type_text("field \"seed\"", "abc");
    page.still();
    assert!(page.find(START).states().has(StateBits::DISABLED));
}

#[test]
fn the_export_button_asks_the_host_to_export_bug_reports() {
    let mut page = no_game("random");
    while page.next_request().is_some() {}
    page.press("button \"Export Bug Reports\"");
    let requests: Vec<Request> = std::iter::from_fn(|| page.next_request()).collect();
    assert_eq!(requests, vec![Request::ExportBugs]);
}

#[test]
fn the_first_view_after_the_agents_shows_the_game() {
    // The live app's session thread sends the agents as soon as it has looked for a game to
    // resume, and a resumed game's first view may reach the client after them.
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    let screen = |client: &mut Client| {
        let frame = client.frame(0);
        agent::tree(frame, &TreeOptions::default()).expect("the whole tree needs no locator")
    };
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 1.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Agents(agents()));
    assert!(screen(&mut client).contains("region \"start\""));
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    assert!(!screen(&mut client).contains("region \"start\""));
}

#[test]
fn the_start_screen_passes_the_checks_at_1440_by_900() {
    let mut page = no_game("random");
    page.check();
}

fn gallery(data: Arc<Data>, size: Size) -> Page {
    Page::in_window(
        data,
        Recording(vec![FixtureMessage::Event {
            event: OPEN.to_owned(),
            payload: "gallery".into(),
        }])
        .into(),
        Side::You,
        Window {
            size,
            ..Window::default()
        },
    )
    .unwrap()
}

#[test]
fn compact_profiles_can_arrange_the_gallery_differently_in_the_same_window() {
    let data = crate::test_data::data();
    let size = Size {
        width: 759.0,
        height: 393.0,
    };
    let mut stacked = (*data).clone();
    stacked.profiles = data.profiles.selecting("phone").unwrap();
    let mut page = gallery(Arc::new(stacked), size);
    page.still();
    let catalog = page.find("list \"Stories\"").rect();
    let preview = page.find("region \"Story preview\"").rect();
    assert!(catalog.y + catalog.height <= preview.y);

    let mut wide = (*data).clone();
    let (_, file) = game_table::profiles::wide_compact::SOURCE;
    let profile = beryl_test_files::read_to_string(format!("data/client/{file}")).unwrap();
    wide.profiles = wide
        .profiles
        .with_preview(file, &profile, wide.card)
        .unwrap();
    page.give_input(Input::Data(Arc::new(wide)));
    page.still();
    let catalog = page.find("list \"Stories\"").rect();
    let preview = page.find("region \"Story preview\"").rect();
    assert!(catalog.x + catalog.width <= preview.x);
}

#[test]
fn the_gallery_plays_stories_in_their_authored_window() {
    use app_client::stories::Source;

    let size = Size {
        width: 1440.0,
        height: 900.0,
    };
    let mut page = gallery(crate::test_data::data(), size).with_stories(
        [
            Source::new("story/kit_ui/example", "story element 200x200 desktop\nbutton \"Sample\" #1 25,25 120x50 | widget=button\n"),
            Source::new("story/kit_fx/sample", "story effect 393x759 phone\neffect glow 48,90 110x140\n"),
        ]
        .into(),
    );
    page.still();
    page.find("list \"Stories\"");
    page.find("region \"Story preview\"");
    page.press("button \"story/kit_ui/example\"");
    let inset = page.frame().inset.as_ref().expect("the selected story");
    assert_eq!(
        inset.size,
        Size {
            width: 200.0,
            height: 200.0
        }
    );
    assert!(
        agent::tree(&inset.frame, &TreeOptions::default())
            .unwrap()
            .contains("button \"Sample\"")
    );
    page.find("text \"desktop · 200 × 200\"");

    page.press("button \"story/kit_fx/sample\"");
    let start = page.frame().inset.as_ref().unwrap().frame.draws.clone();
    page.step(Duration(180_000_000));
    let later = &page.frame().inset.as_ref().unwrap().frame.draws;
    assert_ne!(&start, later);
    page.find("text \"phone · 393 × 759\"");
    page.find("button \"Replay\"");

    page.press("button \"Back\"");
    page.still();
    assert!(page.try_find("region \"Story preview\"").is_err());
    assert!(page.frame().inset.as_ref().unwrap().viewport.width <= 0.0);
}

#[test]
fn opening_the_gallery_from_start_turns_off_music() {
    let mut page = no_game("random");
    assert!(
        std::iter::from_fn(|| page.next_request())
            .any(|r| r == Request::Music(Some(app_screens::screens::Music::Menu)))
    );

    page.chord("G");
    page.still();

    page.find("region \"gallery\"");
    assert!(std::iter::from_fn(|| page.next_request()).any(|r| r == Request::Music(None)));
}

/// Runs `body` on a thread with the 1 MB stack an iPhone gives its main thread, where the iOS
/// host builds the client and draws every frame. The desktop's 8 MB main thread and the
/// simulator's hide a client that overflows it.
fn on_one_megabyte_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(body)
        .unwrap()
        .join()
        .expect("the client fits an iPhone main thread's stack");
}

/// On an iPhone the app crashed as it started: listing the stories made the hidden gallery build
/// its first story, and with the client built on the same stack that overflowed it. The story
/// waits until the gallery is shown, and both fit in 1 MB.
#[test]
fn the_gallery_builds_its_story_only_when_shown_within_a_small_stack() {
    on_one_megabyte_stack(|| {
        let size = Size {
            width: 393.0,
            height: 852.0,
        };
        let mut start = Page::in_window(
            crate::test_data::data(),
            Recording(Vec::new()).into(),
            Side::You,
            Window {
                size,
                ..Window::default()
            },
        )
        .unwrap()
        .with_stories(app_client::stories::embedded());
        start.still();
        assert!(start.frame().inset.is_none());

        let mut page =
            gallery(crate::test_data::data(), size).with_stories(app_client::stories::embedded());
        page.still();
        page.find("region \"Story preview\"");
        assert!(page.frame().inset.is_some());
    });
}
