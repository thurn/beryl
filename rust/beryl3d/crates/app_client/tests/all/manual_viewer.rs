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
use kit_ui::input::Input as DeviceInput;
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
        client.handle(Input::Device(DeviceInput::Pointer(
            kit_ui::input::Pointer {
                id: 0,
                kind: kit_ui::input::PointerKind::Mouse,
                phase,
                position: middle,
                button: kit_ui::input::Button::Primary,
                wheel: glam::Vec2::ZERO,
                modifiers: Modifiers::default(),
            },
        )));
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
    client.handle(Input::Device(DeviceInput::Platform { mac: true }));
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view,
    })));
    client.frame(0);
    // F3 is Search, which looks at the whole library.
    client.handle(Input::Device(DeviceInput::Key(Key {
        code: KeyCode::F3,
        phase: KeyPhase::Down,
        modifiers: Modifiers::default(),
    })));
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
    let group = "Group by type";
    let frame = client.frame(0);
    assert!(
        !button(frame, group)
            .unwrap()
            .state
            .has(kit_frame::frame::StateBits::SELECTED)
    );
    press(&mut client, group);
    let frame = client.frame(0);
    assert!(
        button(frame, group)
            .unwrap()
            .state
            .has(kit_frame::frame::StateBits::SELECTED)
    );
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
    for (width, height, scale) in [(1600.0, 900.0, 2.0), (393.0, 759.0, 3.0)] {
        scroll_pile_and_check_clipping(Size { width, height }, scale);
    }
}

fn scroll_pile_and_check_clipping(size: Size, scale: f32) {
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
    client.handle(Input::Device(DeviceInput::Surface {
        size,
        scale,
        safe_area: Insets::default(),
    }));
    client.handle(Input::Update(Box::new(update)));
    client.frame(0);
    client.dispatch(&Action::new(game_hud::hud::viewer::Message::Open(
        Viewed::Pile(Side::You, ZoneName::Graveyard),
    )));
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
        Input::Device(DeviceInput::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase: PointerPhase::Wheel,
            position: at,
            button: Button::None,
            wheel: Vec2::new(0.0, -distance),
            modifiers: Modifiers::default(),
        }))
    };
    assert!(!top.is_empty());
    client.handle(roll(half_row));
    let frame = client.frame(1_005_000_000);
    assert_clipped_card(frame, id, clip, scale);

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
fn assert_clipped_card(
    frame: &Frame,
    id: kit_frame::frame::ElementId,
    clip: kit_ui::input::Rect,
    scale: f32,
) {
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
                == kit_base::geometry::zoom_rect(clip, (Vec2::ZERO, scale))
    }));
    let shown = |slot: ObjectId| u32::from(slot.0) == id.0;
    let x = bounds.x + bounds.width / 2.0;
    assert!(card_at(frame, Vec2::new(x, clip.y + 5.0), shown).is_some());
    assert!(card_at(frame, Vec2::new(x, clip.y - 5.0), shown).is_none());
}

#[test]
fn a_shown_card_draws_and_hits_inside_its_scaled_hosts_mask() {
    use game_hud::inspect::Viewed;
    use glam::Vec2;
    use kit_frame::frame::{ElementId, Show};
    use kit_ui::element::{Anchor, Gradient, Presence, Stop, column};
    use kit_ui::widgets::screen::screen_timing;

    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    client.handle(Input::Device(DeviceInput::Surface {
        size,
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    let mut builder = ViewBuilder::new();
    let slot = builder.card(ZoneName::Graveyard, Side::You, "Island").slot;
    builder.script(ScriptBuilder::reset().build());
    let mut update = builder.update();
    update.view.table = Some(table(&update.view));
    client.handle(Input::Update(Box::new(update)));
    client.frame(0);
    client.dispatch(&kit_ui::element::Action::new(
        game_hud::hud::viewer::Message::Open(Viewed::Pile(Side::You, ZoneName::Graveyard)),
    ));
    client.frame(1);

    // Keep the same viewer card, but put its host under two transforms and a mask whose
    // box cuts the card. The mask itself is the only clip.
    let object = ElementId(u32::from(slot.0));
    let card = client.data().card.size();
    let mut host = column()
        .name("scaled card host")
        .width(160.0 * card.x / card.y)
        .height(160.0)
        .background([200, 200, 200, 255])
        .on_press(game_hud::hud::viewer::Message::Close);
    host.style.shrink = 0.0;
    host.style.scale = 1.25;
    host.show = Some(Show::object(object));
    let mut mask = column().name("mask").width(180.0).height(100.0).child(host);
    mask.style.mask = Some(Gradient::linear(
        180.0,
        &[Stop::new([0, 0, 0, 255]), Stop::new([0, 0, 0, 64])],
    ));
    let mut panel = column()
        .name("panel")
        .width(200.0)
        .height(180.0)
        .enter(Presence {
            scale: Some(1.0),
            ..Presence::new(screen_timing(180.0))
        })
        .child(mask);
    panel.anchor = Some(Anchor::Point(Vec2::new(500.0, 250.0), Vec2::ZERO));
    panel.style.scale = 1.5;
    client.layer_mut().show(Some(column().child(panel)));
    client.set_looking(true);
    assert_scaled_show(client.frame(1_000_000_000), object, 200.0);
    assert_scaled_show(client.frame(2_000_000_000), object, 300.0);
}

/// The hosted card shares the transformed box and the mask's rectangle at each frame.
fn assert_scaled_show(frame: &Frame, object: kit_frame::frame::ElementId, height: f32) {
    use glam::Vec2;
    use kit_frame::frame::{Band, Material};

    let host = frame
        .show_host(object)
        .expect("the screen registered its Show host");
    let card = frame
        .resolved
        .iter()
        .position(|element| element.role == Role::Card && element.id == object)
        .expect("the viewer drew its card");
    let bounds = |index| kit_scene::hit::bounds(&frame.resolved[index]);
    let host_bounds = bounds(host as usize);
    let card_bounds = bounds(card);
    assert!(
        (host_bounds.height - height).abs() < 0.01,
        "{host_bounds:?}"
    );
    for (actual, expected) in [
        (card_bounds.x, host_bounds.x),
        (card_bounds.y, host_bounds.y),
        (card_bounds.width, host_bounds.width),
        (card_bounds.height, host_bounds.height),
    ] {
        assert!(
            (actual - expected).abs() < 0.01,
            "{card_bounds:?} vs {host_bounds:?}"
        );
    }
    let clip = frame.resolved[host as usize].clip.unwrap();
    assert_eq!(frame.resolved[card].clip, Some(clip));
    assert_ne!(clip, host_bounds);
    let host_clip = frame
        .draws
        .iter()
        .zip(&frame.owners)
        .find_map(|(draw, &owner)| {
            (owner == host && draw.material == Material::Panel as u8).then_some(draw.clip())
        })
        .unwrap();
    let card_clip = frame
        .draws
        .iter()
        .enumerate()
        .find_map(|(index, draw)| {
            (frame.owners[index] == card as u32 && frame.band(index) == Some(Band::LayerCards))
                .then_some(draw.clip())
        })
        .unwrap();
    assert_eq!(card_clip, host_clip);
    assert_ne!(card_clip, 0);
    assert!(frame.paints.table.masks[usize::from(card_clip)].line[2] > 0.0);
    let x = clip.x.max(card_bounds.x) + 10.0;
    let bottom = clip.y + clip.height;
    let pick = |y| {
        game_manual::manual::viewers::card_at(frame, Vec2::new(x, y), |id| {
            u32::from(id.0) == object.0
        })
    };
    assert!(pick(bottom - 5.0).is_some());
    assert!(pick(bottom + 5.0).is_none());
    assert_eq!(
        kit_scene::hit::hit(frame, Vec2::new(x, bottom - 5.0)),
        Some(host)
    );
    assert_ne!(
        kit_scene::hit::hit(frame, Vec2::new(x, bottom + 5.0)),
        Some(host)
    );
}

#[test]
#[should_panic(expected = "a Show host must be laid out before the table")]
fn a_screen_cannot_show_a_card_in_a_box_anchored_to_that_card() {
    use glam::Vec2;
    use kit_frame::frame::{ElementId, Show};
    use kit_ui::element::{Anchor, AnchorTarget, column};

    let mut host = column();
    host.show = Some(Show::object(ElementId(1)));
    host.anchor = Some(Anchor::Object(AnchorTarget::Node(ElementId(1)), Vec2::ZERO));
    kit_ui::layout::Layer::new().show(Some(column().child(host)));
}
