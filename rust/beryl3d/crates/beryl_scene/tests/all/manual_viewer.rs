//! The manual table's pile viewer: its search, grouping and sorting (`manual::viewers`), the
//! mana value it sorts by, and a whole library's search, which the table's answer to a look
//! opens and which asks whether to shuffle before it closes.

use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::ipc::TableActReply;
use beryl_protocol::protocol::{CardType, ObjectId, Side, TableAction, ZoneName};
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::faces::FaceList;
use beryl_scene::frame::{Frame, Role};
use beryl_scene::input::{Input, Insets, Key, KeyCode, KeyPhase, Modifiers, Size};
use beryl_scene::manual::viewers::{Section, Sort, sections};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, table};
use beryl_scene::{Client, Config};

/// A library of five named cards, bottom first, at a solo manual table: Serra Angel at the
/// bottom and Plains on top.
fn library_view() -> beryl_protocol::PlayerView {
    let mut builder = ViewBuilder::new();
    let cards = [
        ("Serra Angel", CardType::Creature),
        ("Island", CardType::Land),
        ("Counterspell", CardType::Instant),
        ("Pacifism", CardType::Enchantment),
        ("Plains", CardType::Land),
    ];
    for (name, kind) in cards {
        let card = builder.card(ZoneName::Library, Side::You, name);
        card.name = Some(name.to_owned());
        card.types = vec![kind];
    }
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    view.table = Some(table(&view));
    view
}

fn faces(view: &beryl_protocol::PlayerView) -> FaceList {
    let names = view.objects.iter().filter_map(|o| o.name.as_deref());
    FaceList::from_records(&card_records_among(names, TokenMakers::NONE))
}

fn names(view: &beryl_protocol::PlayerView, section: &Section) -> Vec<String> {
    (section.cards.iter())
        .map(|slot| {
            let object = view.objects.iter().find(|o| o.slot == *slot).unwrap();
            object.name.clone().unwrap()
        })
        .collect()
}

#[test]
fn a_split_card_s_mana_value_adds_up_both_halves() {
    let records = card_records_among(["Alive // Well", "Counterspell"], TokenMakers::NONE);
    let faces = FaceList::from_records(&records);
    // Alive costs {3}{G} and Well {W}.
    assert_eq!(faces.mana_value("Alive // Well"), Some(5));
    assert_eq!(faces.mana_value("Counterspell"), Some(2));
    assert_eq!(faces.mana_value("Nothing such"), None);
}

#[test]
fn the_viewer_sorts_searches_and_groups_its_cards() {
    let view = library_view();
    let faces = faces(&view);
    let library = view.players.you.library.clone();
    let list =
        |query: &str, sort, grouped| sections(&view, Some(&faces), &library, query, sort, grouped);
    let pile = list("", Sort::Pile, false);
    assert_eq!(
        names(&view, &pile[0]),
        [
            "Plains",
            "Pacifism",
            "Counterspell",
            "Island",
            "Serra Angel"
        ],
        "the pile's order lists the top card first"
    );
    let by_mana = list("", Sort::ManaValue, false);
    assert_eq!(
        names(&view, &by_mana[0]),
        [
            "Island",
            "Plains",
            "Counterspell",
            "Pacifism",
            "Serra Angel"
        ],
        "lands cost nothing, and cards of one mana value go by name"
    );
    let found = list("la", Sort::Name, false);
    assert_eq!(names(&view, &found[0]), ["Island", "Plains"]);
    let grouped = list("", Sort::Pile, true);
    let labels: Vec<_> = grouped.iter().map(|s| s.label.clone().unwrap()).collect();
    assert_eq!(labels, ["Creature", "Enchantment", "Instant", "Land"]);
    assert_eq!(names(&view, &grouped[3]), ["Plains", "Island"]);
}

/// The table actions the client has queued, in order, as JSON.
fn acts(client: &mut Client) -> Vec<serde_json::Value> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::TableAct(action) => Some(serde_json::to_value(action).unwrap()),
            _ => None,
        })
        .collect()
}

fn button<'a>(frame: &'a Frame, name: &str) -> Option<&'a beryl_scene::frame::Resolved> {
    (frame.resolved.iter()).find(|e| e.role == Role::Button && frame.name(e.name) == name)
}

fn press(client: &mut Client, name: &str) {
    let frame = client.frame(0).clone();
    let element = button(&frame, name).unwrap_or_else(|| panic!("no button {name:?}"));
    let middle = element.corners.iter().copied().sum::<glam::Vec2>() / 4.0;
    for phase in [
        beryl_scene::input::PointerPhase::Down,
        beryl_scene::input::PointerPhase::Up,
    ] {
        client.handle(Input::Pointer(beryl_scene::input::Pointer {
            id: 0,
            kind: beryl_scene::input::PointerKind::Mouse,
            phase,
            position: middle,
            button: beryl_scene::input::Button::Primary,
            wheel: glam::Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
    }
}

#[test]
fn closing_a_library_search_asks_whether_to_shuffle_it() {
    let view = library_view();
    let mut client = Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    });
    client.handle(Input::Platform { mac: true });
    client.handle(Input::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view,
    })));
    client.frame(0);
    // F3 is Search, which looks at the whole library.
    client.handle(Input::Key(Key {
        code: KeyCode::F3,
        phase: KeyPhase::Down,
        modifiers: Modifiers::default(),
    }));
    client.frame(0);
    let sent = acts(&mut client);
    assert_eq!(
        sent,
        [serde_json::json!({ "look": { "seat": 0, "portion": "all" } })]
    );
    assert!(
        button(client.frame(0), "close").is_none(),
        "the viewer waits for the table to take the look"
    );
    client.handle(Input::Reply(Reply::TableAct(TableActReply::Ok {
        ok: true,
    })));
    press(&mut client, "close");
    assert!(acts(&mut client).is_empty(), "the first close only asks");
    let frame = client.frame(0).clone();
    let shuffle = button(&frame, "Shuffle").expect("Shuffle is offered");
    assert!(shuffle.state.has(beryl_scene::frame::StateBits::PRIMARY));
    assert!(button(&frame, "Don't shuffle").is_some());
    press(&mut client, "Shuffle");
    assert_eq!(
        acts(&mut client),
        [serde_json::json!({ "batch": { "actions": [
            { "shuffle": { "seat": 0, "portion": "all" } },
            { "close_look": {} },
        ] } })]
    );
    assert!(
        button(client.frame(0), "Shuffle").is_none(),
        "the viewer closed"
    );
}

#[test]
fn a_look_at_the_top_lists_only_the_cards_it_took() {
    let view = library_view();
    let action: TableAction = serde_json::from_value(
        serde_json::json!({ "look": { "seat": 0, "portion": { "top": 2 } } }),
    )
    .unwrap();
    let look = beryl_scene::manual::viewers::Look::of(&action, &view).unwrap();
    assert_eq!(look.cards(&view), [ObjectId(3), ObjectId(4)]);
    // A card that arrives on top later is not part of the look.
    let mut later = view.clone();
    later.players.you.library.push(ObjectId(9));
    assert_eq!(look.cards(&later), [ObjectId(4)]);
}
