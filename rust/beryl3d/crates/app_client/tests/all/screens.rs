//! The start screen, driven through the `Page` test driver: choosing an opponent, starting a
//! game, conceding a game in progress first, and the screen's place at startup.

use app_client::Input;
use app_client::command::{Reply, Request};
use app_client::driver::{OPEN, Page, Recording};
use app_client::{Client, Config};
use beryl_protocol::ipc::{CONCEDE, ConcedeRequest, FixtureMessage, LIST_AGENTS, START_GAME};
use beryl_protocol::protocol::{Side, Status};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use beryl_protocol::{AgentView, SessionId, StartRequest};
use kit_frame::frame::StateBits;
use kit_probe::agent::{self, TreeOptions};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

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
    page_at(messages, size)
}

fn page_at(messages: Vec<FixtureMessage>, size: Size) -> Page {
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::REGULAR,
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
fn pressing_start_with_the_second_agent_chosen_starts_against_it() {
    let mut page = no_game("greedy");
    assert!(
        page.find(START).states().has(StateBits::PRIMARY),
        "the tree marks Start as the button the screen expects"
    );
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

fn gallery(width: f32, height: f32) -> Page {
    page_at(
        vec![FixtureMessage::Event {
            event: OPEN.to_owned(),
            payload: "gallery".into(),
        }],
        Size { width, height },
    )
}

fn scroll_gallery(page: &mut Page, distance: f32) {
    use glam::Vec2;
    use kit_ui::input::{Button, Modifiers, Pointer, PointerKind, PointerPhase};

    page.give_input(Input::Device(DeviceInput::Pointer(Pointer {
        kind: PointerKind::Mouse,
        phase: PointerPhase::Wheel,
        position: Vec2::new(720.0, 450.0),
        button: Button::None,
        wheel: Vec2::new(0.0, -distance),
        id: 0,
        modifiers: Modifiers::default(),
    })));
    page.still();
}

#[test]
fn the_gallery_contains_every_section_and_scrolls_to_the_command_list() {
    let mut page = gallery(1440.0, 900.0);
    let visited = agent::tree(
        page.frame(),
        &TreeOptions {
            full: true,
            ..TreeOptions::default()
        },
    )
    .unwrap();
    for (index, heading) in [
        "Foundations",
        "Actions & status",
        "Player & zones",
        "Cards & states",
        "Combat & stack",
        "Scry overlay",
        "Manual table prompts",
        "Context menus",
        "Command list",
    ]
    .into_iter()
    .enumerate()
    {
        // Each panel's title is numbered, as on the web's gallery page.
        let title = format!("{:02} {heading}", index + 1);
        assert!(visited.contains(&format!("text \"{title}\"")), "{title}");
    }
    let last = "button \"No matches\"";
    for (width, height) in [
        (1440.0, 900.0),
        (1600.0, 900.0),
        (1920.0, 1080.0),
        (2560.0, 1080.0),
    ] {
        let mut page = gallery(width, height);
        page.check();
        assert!(!page.find(last).interactive);
        scroll_gallery(&mut page, 10_000.0);
        assert!(page.find(last).interactive);
        page.check();
    }
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

#[test]
fn examples_open_their_menu_and_command_preview() {
    let mut page = gallery(1440.0, 900.0);
    scroll_gallery(&mut page, 10_000.0);
    page.press("button \"Bottom-right corner\"");
    page.still();
    page.find("menuitem \"Keep top card revealed\"");
    page.check();

    page.chord("Escape");
    page.still();
    assert!(
        page.try_find("menuitem \"Keep top card revealed\"")
            .is_err()
    );
    page.press("button \"No matches\"");
    scroll_gallery(&mut page, 10_000.0);
    page.find("text \"No matching commands\"");
}

#[test]
fn the_wheel_scrolls_a_long_opponent_list_to_its_last_agent() {
    // The host offers a dozen agents, more than the list has room for at 1600 by 900.
    let many: Vec<AgentView> = (0..12)
        .map(|at| AgentView {
            name: format!("agent-{at}"),
            title: format!("Agent {at}"),
            description: format!("The agent numbered {at}"),
        })
        .collect();
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut page = page_at(vec![FixtureMessage::response(LIST_AGENTS, &many)], size);
    let last = page.try_act(&"press toggle \"Agent 11\"".parse().unwrap());
    assert!(
        last.is_err(),
        "the last agent is out of sight before the list scrolls"
    );
    page.run("wheel list \"Opponent\" 0,-2000\nstill");
    page.press("toggle \"Agent 11\"");
    page.still();
    assert!(
        page.find("toggle \"Agent 11\"")
            .states()
            .has(StateBits::SELECTED)
    );
}
