//! Screen recordings: a client put on a screen, or a sheet opened over one, with no engine and
//! an empty view, as the stress boards are made (`crate::stress`). Each is named
//! `screen/<name>`, and `data/beryl3d/screens.json` lists them for the parity list. The manual
//! table's is a view of a solo table built by hand, with a few cards in each zone and a
//! face-down card on each side's battlefield, which opens the table screen itself, and then the
//! same view again with the banner a shuffle announces. The lobby's is a solo table's lobby
//! built by hand, with the sample
//! deck loaded. The manual table's assists have one of their own, `screen/table-assists`: the
//! same table with every assist on, a turn upkeep proposal, a state nudge's label on the
//! opponent's creature and an activated ability on the viewer's first land.
//! The other `screen/table-*` recordings are the same table with something more: the guided
//! start's play or draw choice (`first`), the opening hand after a mulligan with a card to put
//! on the bottom (`opening`), a hosted table the other player left (`left`), and a table where
//! the viewer can search their library (`search`): their library's cards are named, as the
//! table shows them to a player who looks, the table takes the look the viewer asks for, and
//! the opponent is looking at the top three cards of their own library.
//! `screen/start-tables` is the start screen with a saved hosted table and the last table the
//! viewer joined, whose recorded answer to joining a table is the version mismatch a guest
//! with another version of Beryl is refused with.

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::ipc::{
    CARD_RECORDS, FixtureMessage, LIST_AGENTS, SavedTable, TABLE_ACT, TABLE_JOIN, TABLE_LOBBY,
    TABLE_SAVED, TableActReply, TableConnectionError,
};
use beryl_protocol::protocol::{
    AssistsView, CardHandling, CardType, Facing, LabelView, LinkState, LogLine, PlayerState,
    ProposalLine, ProposalView, Side, TableAbilityOption, TableCardCommands, TablePrompt,
    TableView, TargetRef, ZoneName,
};
use beryl_protocol::script::{Act, Notice, Reset, Step, StepTiming};
use beryl_protocol::{AgentView, GameUpdate, PlayerView, SessionId};
use beryl_scene::driver::{OPEN, RECORDINGS};
use beryl_scene::testing;

/// What every screen recording's name starts with.
pub const PREFIX: &str = "screen/";

/// The name of every screen recording.
pub const NAMES: [&str; 13] = [
    "screen/start",
    "screen/start-tables",
    "screen/lobby",
    "screen/table",
    "screen/table-assists",
    "screen/table-first",
    "screen/table-opening",
    "screen/table-left",
    "screen/table-search",
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
        "start-tables" => vec!["start"],
        // A table's view opens the table screen; the start screen's press would start a table.
        _ if table_screen(opened) => Vec::new(),
        _ => vec![opened],
    };
    let mut view = if table_screen(opened) {
        let mut builder = testing::ViewBuilder::sample();
        // UI Conventions, "Inspecting a face-down manual card": each side has a face-down
        // card on the battlefield, which the table stands after its other permanents. The
        // viewer knows their own, so its name is in the view and pointing at it shows its face;
        // the opponent's has no name, so pointing at it shows nothing.
        for (side, name) in [(Side::You, "Serra Angel"), (Side::Opponent, "")] {
            let card = builder.card(ZoneName::Battlefield, side, name);
            card.name = (!name.is_empty()).then(|| name.to_owned());
            card.facing = Facing::Down;
        }
        let mut view = builder.build();
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
        if opened == "table-search" {
            searched(&mut view);
        }
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
    if opened == "table-search" {
        messages.push(FixtureMessage::response(
            TABLE_ACT,
            &TableActReply::Ok { ok: true },
        ));
    }
    if opened == "start-tables" {
        messages.extend(saved_tables());
    }
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

/// The recorded answers of `screen/start-tables`: two saved tables, a hosted one and the last
/// table the viewer joined (listed first, as `TableConnections::saved` lists it), and a join
/// refused because the host has another version of Beryl.
fn saved_tables() -> [FixtureMessage; 2] {
    let saved =
        [("joined", "Sam", Some("Alex")), ("5f3a9c", "Alex", None)].map(|(id, host, guest)| {
            SavedTable {
                id: id.to_owned(),
                host: host.to_owned(),
                guest: guest.map(str::to_owned),
            }
        });
    // The words `beryl_link`'s handshake refuses a guest of another version with
    // (`wire::VERSION_MISMATCH`), which this crate does not link.
    let mismatch = TableConnectionError {
        reason: "Both players need the same version of Beryl.".to_owned(),
    };
    [
        FixtureMessage::response(TABLE_SAVED, &saved),
        FixtureMessage::response(TABLE_JOIN, &mismatch),
    ]
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

/// Names the cards of the viewer's library in `view`, a few different ones so that a search of
/// it can be sorted and grouped, and has the opponent look at the top three cards of theirs.
fn searched(view: &mut PlayerView) {
    let names = [
        "Serra Angel",
        "Island",
        "Counterspell",
        "Pacifism",
        "Plains",
    ];
    let library = view.players.you.library.clone();
    for (slot, name) in library.iter().zip(names.iter().cycle()) {
        if let Some(object) = view.objects.iter_mut().find(|object| object.slot == *slot) {
            object.name = Some((*name).to_owned());
            object.types = match *name {
                "Serra Angel" => vec![CardType::Creature],
                "Pacifism" => vec![CardType::Enchantment],
                "Counterspell" => vec![CardType::Instant],
                _ => vec![CardType::Land],
            };
        }
    }
    view.handling = Some(CardHandling::Pile {
        side: Side::Opponent,
        pile: ZoneName::Library,
        count: 3,
        bottom: false,
    });
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
