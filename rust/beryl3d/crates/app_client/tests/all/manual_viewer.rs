//! The manual table's pile viewer: its search, grouping and sorting (`manual::viewers`), the
//! mana value it sorts by, and a whole library's search, which the table's answer to a look
//! opens and which asks whether to shuffle before it closes.

use app_client::Input;
use app_client::command::{Reply, SessionCommand};
use app_client::{Client, Config};
use beryl_card_records::{TokenMakers, card_records_among};
use beryl_protocol::ipc::TableActReply;
use beryl_protocol::protocol::{CardType, ObjectId, Side, TableAction, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, table};
use game_manual::manual::viewers::{Section, Sort, sections};
use game_table::faces::FaceList;
use kit_frame::frame::{Frame, Role};
use kit_ui::input::{Insets, Key, KeyCode, KeyPhase, Modifiers, Size};

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

fn button<'a>(frame: &'a Frame, name: &str) -> Option<&'a kit_frame::frame::Resolved> {
    (frame.resolved.iter()).find(|e| e.role == Role::Button && frame.name(e.name) == name)
}

fn press(client: &mut Client, name: &str) {
    let frame = client.frame(0).clone();
    let element = button(&frame, name).unwrap_or_else(|| panic!("no button {name:?}"));
    let middle = element.corners.iter().copied().sum::<glam::Vec2>() / 4.0;
    for phase in [
        kit_ui::input::PointerPhase::Down,
        kit_ui::input::PointerPhase::Up,
    ] {
        client.handle(Input::Pointer(kit_ui::input::Pointer {
            id: 0,
            kind: kit_ui::input::PointerKind::Mouse,
            phase,
            position: middle,
            button: kit_ui::input::Button::Primary,
            wheel: glam::Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
    }
}

#[test]
fn closing_a_library_search_asks_whether_to_shuffle_it() {
    let view = library_view();
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 2048,
            draws: 2048,
            name_bytes: 65_536,
        },
    );
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
    assert!(shuffle.state.has(kit_frame::frame::StateBits::PRIMARY));
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
    let look = game_manual::manual::viewers::Look::of(&action, &view).unwrap();
    assert_eq!(look.cards(&view), [ObjectId(3), ObjectId(4)]);
    // A card that arrives on top later is not part of the look.
    let mut later = view.clone();
    later.players.you.library.push(ObjectId(9));
    assert_eq!(look.cards(&later), [ObjectId(4)]);
}

#[test]
fn scrolling_a_large_pile_moves_its_cards_and_clips_cards_that_leave_the_grid() {
    use app_client::Press;
    use game_hud::inspect::Viewed;
    use glam::Vec2;
    use kit_ui::element::Action;
    use kit_ui::input::{Button, Pointer, PointerKind, PointerPhase};

    let mut builder = ViewBuilder::new();
    for number in 0..100 {
        builder.card(ZoneName::Graveyard, Side::You, &format!("Discard {number}"));
    }
    builder.script(ScriptBuilder::reset().build());
    let mut update = builder.update();
    update.view.table = Some(table(&update.view));

    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 2048,
            draws: 2048,
            name_bytes: 65_536,
        },
    );
    client.handle(Input::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Update(Box::new(update)));
    client.frame(0);
    client.press(&Action::new(Press::View(Viewed::Pile(
        Side::You,
        ZoneName::Graveyard,
    ))));
    client.frame(0);
    let frame = client.frame(1_000_000_000);
    let grid = frame
        .resolved
        .iter()
        .find(|element| element.role == Role::List && frame.name(element.name) == "cards")
        .expect("the pile viewer shows a grid");
    let at = grid.corners.iter().copied().sum::<Vec2>() / 4.0;
    let cards = |frame: &Frame| -> Vec<_> {
        frame
            .resolved
            .iter()
            .filter(|element| element.role == Role::Card && element.opacity > 0.0)
            .map(|element| element.id)
            .collect()
    };
    let top = cards(frame);
    let card = frame
        .resolved
        .iter()
        .filter(|element| element.role == Role::Card && element.clip.is_some())
        .min_by(|a, b| {
            kit_scene::hit::bounds(a)
                .y
                .total_cmp(&kit_scene::hit::bounds(b).y)
        })
        .expect("a card takes its clip from the grid");
    let clip = card.clip.unwrap();
    let bounds = kit_scene::hit::bounds(card);
    let half_row = bounds.y - clip.y + bounds.height / 2.0;
    let id = card.id;
    let roll = |distance: f32| {
        Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase: PointerPhase::Wheel,
            position: at,
            button: Button::None,
            wheel: Vec2::new(0.0, -distance),
            modifiers: Modifiers::default(),
        })
    };
    assert!(!top.is_empty());
    client.handle(roll(half_row));
    let frame = client.frame(1_005_000_000);
    assert_clipped_card(frame, id, clip);

    client.handle(roll(10_000.0));
    let end = cards(client.frame(1_010_000_000));
    assert!(!end.is_empty());
    assert!(
        top.iter().any(|id| !end.contains(id)),
        "cards that left the clip disappeared"
    );
    assert!(
        end.iter().any(|id| !top.contains(id)),
        "new cards scrolled into view"
    );
}

/// The same rectangle cuts the card in drawing and in the manual viewer's hit test.
fn assert_clipped_card(frame: &Frame, id: kit_frame::frame::ElementId, clip: kit_ui::input::Rect) {
    use game_manual::manual::viewers::card_at;
    use glam::Vec2;

    let card = frame.resolved.iter().find(|card| card.id == id).unwrap();
    let bounds = kit_scene::hit::bounds(card);
    assert!(bounds.y < clip.y && bounds.y + bounds.height > clip.y);
    assert!((0..frame.resolved.len()).any(|at| frame.shown_by(at as u32) == Some(id)));
    assert!(frame.draws.iter().enumerate().any(|(at, draw)| {
        frame.band(at) == Some(kit_frame::frame::Band::LayerCards)
            && draw.clip() > 0
            && frame.clips[usize::from(draw.clip())]
                == kit_base::geometry::zoom_rect(clip, (Vec2::ZERO, 2.0))
    }));
    let shown = |slot: ObjectId| u32::from(slot.0) == id.0;
    let x = bounds.x + bounds.width / 2.0;
    assert!(card_at(frame, Vec2::new(x, clip.y + 5.0), shown).is_some());
    assert!(card_at(frame, Vec2::new(x, clip.y - 5.0), shown).is_none());
}
