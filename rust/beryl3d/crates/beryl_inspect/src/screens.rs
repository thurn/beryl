//! Screen recordings: a client put on a screen, or a sheet opened over one, with no engine and
//! an empty view, as the stress boards are made (`crate::stress`). Each is named
//! `screen/<name>`, and `data/beryl3d/screens.json` lists them for the parity list. The manual
//! table's is a view of a solo table built by hand, with a few cards in each zone, which opens
//! the table screen itself, and then the same view again with the banner a shuffle announces.
//! The lobby's is a solo table's lobby built by hand, with the sample
//! deck loaded. The manual table's assists have one of their own, `screen/table-assists`: the
//! same table with every assist on, a turn upkeep proposal, a state nudge's label on the
//! opponent's creature and an activated ability on the viewer's first land.
//! The other `screen/table-*` recordings are the same table with something more: the guided
//! start's play or draw choice (`first`), the opening hand after a mulligan with a card to put
//! on the bottom (`opening`), and a hosted table the other player left (`left`).

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage, LIST_AGENTS, TABLE_LOBBY};
use beryl_protocol::protocol::{
    AssistsView, CardType, LabelView, LinkState, LogLine, PlayerState, ProposalLine, ProposalView,
    Side, TableAbilityOption, TableCardCommands, TablePrompt, TableView, TargetRef, ZoneName,
};
use beryl_protocol::script::{Act, Notice, Reset, Step, StepTiming};
use beryl_protocol::{AgentView, GameUpdate, PlayerView, SessionId};
use beryl_scene::driver::{OPEN, RECORDINGS};
use beryl_scene::testing;

/// What every screen recording's name starts with.
pub const PREFIX: &str = "screen/";

/// The name of every screen recording.
pub const NAMES: [&str; 11] = [
    "screen/start",
    "screen/lobby",
    "screen/table",
    "screen/table-assists",
    "screen/table-first",
    "screen/table-opening",
    "screen/table-left",
    "screen/gallery",
    "screen/testing",
    "screen/settings",
    "screen/bug-report",
];

/// The agents the screen recordings offer: three of the host's, one of which cheats, so that the
/// start screen shows each kind of row.
fn agents() -> Vec<AgentView> {
    [
        (
            "mcts-omniscient-quick",
            "Quick Seer",
            "Cheats as the Seer does, answering in a moment",
        ),
        (
            "mcts-hidden",
            "Searcher",
            "Searches many guesses at the cards it cannot see, playing each out greedily",
        ),
        (
            "expert",
            "Expert",
            "Plays by a strong player's rules for this deck, and works out combat",
        ),
    ]
    .map(|(name, title, description)| AgentView {
        name: name.to_owned(),
        title: title.to_owned(),
        description: description.to_owned(),
    })
    .to_vec()
}

/// The messages of the screen recording `name`, or none for any other name: the records of no
/// cards, the agents a person may play, the event that opens the screen or sheet (over the
/// start screen), the lobby's state for the lobby, and one view: empty, or the hand-built
/// table's.
#[must_use]
pub fn messages(name: &str) -> Option<Vec<FixtureMessage>> {
    if !NAMES.contains(&name) {
        return None;
    }
    let opened = name.strip_prefix(PREFIX)?;
    let opens = match opened {
        "settings" | "bug-report" => vec!["start", opened],
        // A table's view opens the table screen; the start screen's press would start a table.
        _ if table_screen(opened) => Vec::new(),
        _ => vec![opened],
    };
    let mut view = if table_screen(opened) {
        let mut view = testing::ViewBuilder::sample().build();
        let mut table = testing::table(&view);
        // A few log lines, so that the checks see the log at the table's left with its undo
        // and rewind buttons.
        table.log = [
            (1, "Shuffled your library.", false),
            (2, "Drew 7 cards.", false),
            (3, "Drew a card.", true),
        ]
        .map(|(n, text, undoable)| LogLine {
            n,
            by: "You".to_owned(),
            text: text.to_owned(),
            struck: false,
            undoable,
            blocked_by: Vec::new(),
        })
        .into();
        if opened == "table-assists" {
            assists(&view, &mut table);
        }
        guided_or_linked(opened, &mut table);
        view.table = Some(table);
        view
    } else {
        PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20))
    };
    view.script = Some(testing::script(
        Some(Reset::default()),
        Vec::new(),
        Vec::new(),
    ));
    // The table's cards have placeholder faces, as a stress board's do.
    let records = if table_screen(opened) {
        crate::stress::placeholder_records(std::slice::from_ref(&view))
    } else {
        CardRecords::none()
    };
    let mut messages = vec![
        FixtureMessage::response(CARD_RECORDS, &records),
        FixtureMessage::response(LIST_AGENTS, &agents()),
    ];
    if opened == "testing" {
        messages.push(FixtureMessage::Event {
            event: RECORDINGS.to_owned(),
            payload: crate::recordings::listed().unwrap_or_default().into(),
        });
    }
    messages.extend(opens.into_iter().map(|opened| FixtureMessage::Event {
        event: OPEN.to_owned(),
        payload: opened.into(),
    }));
    if opened == "lobby" {
        messages.push(FixtureMessage::Event {
            event: TABLE_LOBBY.to_owned(),
            payload: serde_json::to_value(testing::lobby(true)).ok()?,
        });
    }
    let shuffled = (opened == "table").then(|| shuffled(&view));
    messages.push(FixtureMessage::game_update(&GameUpdate {
        session: SessionId(1),
        view,
    }));
    messages.extend(shuffled.map(|view| {
        FixtureMessage::game_update(&GameUpdate {
            session: SessionId(1),
            view,
        })
    }));
    Some(messages)
}

/// Turns every assist of `table`, a table of `view`, on, with what each shows: a turn upkeep
/// proposal of two lines, a state nudge's label on the opponent's creature, and an activated
/// ability on the viewer's first land, each worded as the table words them.
fn assists(view: &PlayerView, table: &mut TableView) {
    let slot = |side: Side, zone: ZoneName| {
        (view.objects.iter())
            .find(|object| {
                object.controller == side && object.zone == zone && !object.types.is_empty()
            })
            .map(|object| object.slot)
    };
    let creature = (view.objects.iter())
        .find(|object| {
            object.controller == Side::Opponent && object.types.contains(&CardType::Creature)
        })
        .map(|object| object.slot);
    table.assists = AssistsView {
        turn_upkeep: true,
        combat: true,
        state_nudges: true,
        library_flows: true,
        ability_menus: true,
    };
    table.proposals = vec![ProposalView {
        id: 1,
        revision: 1,
        title: "Turn upkeep".to_owned(),
        lines: [(1, "Untap Island", true), (2, "Draw a card", true)]
            .map(|(id, text, on)| ProposalLine {
                id,
                text: text.to_owned(),
                whose: Side::You,
                on,
            })
            .into(),
    }];
    table.labels = creature
        .map(|slot| LabelView {
            id: 1,
            on: TargetRef::Object { slot },
            text: "Move to graveyard".to_owned(),
        })
        .into_iter()
        .collect();
    if let Some(land) = slot(Side::You, ZoneName::Battlefield) {
        table.card_commands.insert(
            land,
            TableCardCommands {
                base_stats: None,
                counter_names: Default::default(),
                auto_attach: false,
                flows: Vec::new(),
                abilities: vec![TableAbilityOption {
                    label: "{T}: Add {U}.".to_owned(),
                    requires_x: false,
                }],
            },
        );
    }
}

/// Whether the screen recording `opened` (its name after `PREFIX`) is a hand-built table's view.
fn table_screen(opened: &str) -> bool {
    opened.starts_with("table")
}

/// Gives the hand-built `table` what the screen recording `opened` adds to it: the
/// guided start's play or draw choice, its opening hand after one mulligan, or a link to a
/// guest who has left.
fn guided_or_linked(opened: &str, table: &mut TableView) {
    match opened {
        "table-first" => {
            table.choosing_first = true;
            table.prompt = Some(TablePrompt::Choose {
                title: "You won the roll".to_owned(),
                options: vec!["Play".to_owned(), "Draw".to_owned()],
                selected: None,
            });
        }
        "table-opening" => {
            table.prompt = Some(TablePrompt::Opening {
                hand: table.orders.you.hand.clone(),
                mulligans: 1,
                to_bottom: Some(1),
                bottom: Vec::new(),
            });
        }
        "table-left" => {
            table.link = LinkState::Left;
            table.peer_name = Some("Alex".to_owned());
        }
        _ => {}
    }
}

/// `view` again, with a script that announces a shuffle, as a manual table's Shuffle library
/// sends it: the banner shows over the table for its step, as the table's first view's script
/// never plays (`play::compile`).
fn shuffled(view: &PlayerView) -> PlayerView {
    let step = Step {
        act: Act::Announce {
            notice: Notice::Shuffle,
        },
        cause: None,
    };
    let timing = StepTiming {
        delay: 116.0,
        passes: true,
        ..testing::timing(1200.0)
    };
    PlayerView {
        script: Some(testing::script(None, vec![vec![step]], vec![timing])),
        ..view.clone()
    }
}
