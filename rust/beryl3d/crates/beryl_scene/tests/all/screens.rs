//! The start screen, driven through the `Page` test driver: choosing an opponent, starting a
//! game, conceding a game in progress first, and the screen's place at startup.

use beryl_protocol::ipc::{CONCEDE, ConcedeRequest, FixtureMessage, LIST_AGENTS, START_GAME};
use beryl_protocol::protocol::{Side, Status};
use beryl_protocol::{AgentView, SessionId, StartRequest};
use beryl_scene::agent::{self, Layout, TreeOptions};
use beryl_scene::command::Request;
use beryl_scene::driver::{OPEN, Page, Recording};
use beryl_scene::frame::StateBits;
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::{Client, Config};

const START: &str = "button \"Start\"";

/// Three agents, none of them the one offered first, so that the first is chosen at the start.
fn agents() -> Vec<AgentView> {
    ["random", "greedy", "expert"]
        .map(|name| AgentView {
            name: name.to_owned(),
            title: name.to_uppercase(),
            description: format!("The {name} agent"),
        })
        .to_vec()
}

/// A start request against `agent` with no seed and no fair deal.
fn request(agent: &str) -> StartRequest {
    StartRequest {
        agent: agent.to_owned(),
        seed: None,
        fair: false,
    }
}

/// `messages` played at 1440 by 900, settled.
fn page(messages: Vec<FixtureMessage>) -> Page {
    let size = Size {
        width: 1440.0,
        height: 900.0,
    };
    let mut page = Page::new(Recording(messages).into(), Side::You, Layout::Desktop, size).unwrap();
    page.still();
    page
}

/// How many answers to commands the client has been given so far.
fn replies(page: &Page) -> usize {
    (page.inputs().iter())
        .filter(|input| matches!(input, Input::Reply(_)))
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
fn pressing_start_with_the_second_agent_chosen_starts_against_it() {
    let mut page = no_game("greedy");
    assert!(
        page.find("toggle \"RANDOM\"")
            .states()
            .has(StateBits::SELECTED)
    );
    page.press("toggle \"GREEDY\"");
    page.still();
    assert!(
        page.find("toggle \"GREEDY\"")
            .states()
            .has(StateBits::SELECTED)
    );
    page.press(START);
    page.still();
    // A start for any other agent would not match the recorded request and fail the page.
    assert_eq!(replies(&page), 1, "the start was sent and answered");
    assert!(
        page.try_find(START).is_err(),
        "the game screen replaced the start screen"
    );
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
    let mut client = Client::new(Config::LARGEST);
    let screen = |client: &mut Client| {
        let frame = client.frame(0);
        agent::tree(frame, &TreeOptions::default()).expect("the whole tree needs no locator")
    };
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 1.0,
        safe_area: Insets::default(),
    });
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
