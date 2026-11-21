//! The manual table's assists, on views built by hand: the proposal panel's lines and Apply,
//! a state nudge's label, the abilities a permanent's menu lists, Scry and Surveil in the
//! library's and a card's menu, and the sorting tray. Each answers the table with
//! `SessionCommand::TableAnswer`.

use app_client::Input;
use app_client::command::SessionCommand;
use app_client::{Client, Config};
use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{
    Answer, AssistsView, LabelView, ObjectId, PileName, ProposalLine, ProposalView, Side,
    TableAbilityOption, TableCardCommands, TableFlowOption, TablePrompt, TargetRef, ZoneName,
};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, table};
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits};
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
    TextChange,
};

/// A sample view at a solo manual table with every assist on, at rest.
fn assisted_view() -> PlayerView {
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let mut view = builder.build();
    let mut table = table(&view);
    table.assists = AssistsView {
        turn_upkeep: true,
        combat: true,
        state_nudges: true,
        library_flows: true,
        ability_menus: true,
    };
    view.table = Some(table);
    view
}

/// A client on the Mac in a 1600 by 900 window that has been given `view` and drawn it.
fn client_at(view: &PlayerView) -> Client {
    client_sized(view, 1600.0, 900.0)
}

/// A client on the Mac in a window `width` by `height` that has been given `view` and drawn it.
fn client_sized(view: &PlayerView, width: f32, height: f32) -> Client {
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
        size: Size { width, height },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::SessionId(1),
        view: view.clone(),
    })));
    client.frame(0);
    client
}

/// The first element of `frame` with `role` whose name starts with `name`, if any.
fn find<'a>(frame: &'a Frame, role: Role, name: &str) -> Option<&'a kit_frame::frame::Resolved> {
    (frame.resolved.iter())
        .find(|element| element.role == role && frame.name(element.name).starts_with(name))
}

/// The middle of the first element drawn at `now` with `role` whose name starts with `name`.
fn middle(client: &mut Client, now: u64, role: Role, name: &str) -> Vec2 {
    let frame = client.frame(now).clone();
    let element = find(&frame, role, name).unwrap_or_else(|| {
        let names: Vec<&str> = (frame.resolved.iter())
            .filter(|element| element.role == role)
            .map(|element| frame.name(element.name))
            .collect();
        panic!("no {role:?} named {name:?} among {names:?}")
    });
    element.corners.iter().copied().sum::<Vec2>() / 4.0
}

fn click(client: &mut Client, at: Vec2, button: Button) {
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        client.handle(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: at,
            button,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
    }
}

/// Presses the first element drawn at `now` with `role` whose name starts with `name`.
fn press(client: &mut Client, now: u64, role: Role, name: &str) {
    let at = middle(client, now, role, name);
    click(client, at, Button::Primary);
}

/// Opens the context menu of the first element drawn at `now` with `role` named so, and
/// returns the names of its rows.
fn menu(client: &mut Client, now: u64, role: Role, name: &str) -> Vec<String> {
    let at = middle(client, now, role, name);
    click(client, at, Button::Secondary);
    let frame = client.frame(now + 1);
    (frame.resolved.iter())
        .filter(|element| element.role == Role::MenuItem)
        .map(|element| frame.name(element.name).to_owned())
        .collect()
}

/// The answers the client has queued, in order.
fn answers(client: &mut Client) -> Vec<Answer> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::TableAnswer(answer) => Some(answer),
            _ => None,
        })
        .collect()
}

/// The table actions the client has queued, as JSON.
fn acts(client: &mut Client) -> Vec<serde_json::Value> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::TableAct(action) => serde_json::to_value(action).ok(),
            _ => None,
        })
        .collect()
}

/// The slot of the first card named `name` in `side`'s `zone` of `view`.
fn slot_of(view: &PlayerView, zone: ZoneName, side: Side, name: &str) -> ObjectId {
    (view.objects.iter())
        .find(|object| {
            object.zone == zone && object.controller == side && object.name.as_deref() == Some(name)
        })
        .unwrap_or_else(|| panic!("no {name:?} in {side:?}'s {zone:?}"))
        .slot
}

/// `view` with the abilities and library instructions of `slot`'s visible face.
fn with_commands(
    mut view: PlayerView,
    slot: ObjectId,
    abilities: &[(&str, bool)],
    flows: &[TableFlowOption],
) -> PlayerView {
    let table = view.table.as_mut().unwrap();
    table.card_commands.insert(
        slot,
        TableCardCommands {
            base_stats: None,
            counter_names: Default::default(),
            auto_attach: false,
            flows: flows.to_vec(),
            abilities: (abilities.iter())
                .map(|&(label, requires_x)| TableAbilityOption {
                    label: label.to_owned(),
                    requires_x,
                })
                .collect(),
        },
    );
    view
}

#[test]
fn a_proposal_line_turns_off_and_apply_sends_the_proposal() {
    let mut view = assisted_view();
    view.table.as_mut().unwrap().proposals = vec![ProposalView {
        id: 7,
        revision: 3,
        title: "Turn upkeep".to_owned(),
        lines: vec![ProposalLine {
            id: 1,
            text: "Draw a card".to_owned(),
            whose: Side::You,
            on: true,
        }],
    }];
    let mut client = client_at(&view);
    let frame = client.frame(1).clone();
    let line = find(&frame, Role::Toggle, "Draw a card · yours").expect("no line");
    assert!(
        line.state.has(StateBits::SELECTED),
        "the line is not checked"
    );

    press(&mut client, 1, Role::Toggle, "Draw a card");
    press(&mut client, 2, Role::Button, "Apply");
    assert_eq!(
        answers(&mut client),
        [
            Answer::ProposalLine {
                id: 7,
                line: 1,
                on: false
            },
            Answer::ApplyProposal { id: 7, revision: 3 },
        ]
    );
}

#[test]
fn a_phone_folds_the_proposal_panel_to_a_button_that_opens_it() {
    let mut view = assisted_view();
    view.table.as_mut().unwrap().proposals = vec![ProposalView {
        id: 7,
        revision: 3,
        title: "Turn upkeep".to_owned(),
        lines: vec![ProposalLine {
            id: 1,
            text: "Draw a card".to_owned(),
            whose: Side::You,
            on: true,
        }],
    }];
    let mut client = client_sized(&view, 393.0, 759.0);
    let frame = client.frame(1).clone();
    assert!(
        find(&frame, Role::Button, "Apply").is_none(),
        "the panel is open"
    );

    press(&mut client, 1, Role::Button, "Show proposals");
    press(&mut client, 2, Role::Button, "Apply");
    assert_eq!(
        answers(&mut client),
        [Answer::ApplyProposal { id: 7, revision: 3 }]
    );
    let frame = client.frame(3).clone();
    assert!(
        find(&frame, Role::Button, "Show proposals").is_some(),
        "Apply did not fold the panel"
    );
}

#[test]
fn a_nudge_label_on_a_creature_applies_its_action() {
    let mut view = assisted_view();
    let angel = slot_of(&view, ZoneName::Battlefield, Side::Opponent, "Serra Angel");
    let you = slot_of(&view, ZoneName::Hand, Side::You, "Counterspell");
    view.table.as_mut().unwrap().labels = vec![
        LabelView {
            id: 4,
            on: TargetRef::Object { slot: angel },
            text: "Move to graveyard".to_owned(),
        },
        // A card in a hand shows no label, as the web shows none off the battlefield.
        LabelView {
            id: 5,
            on: TargetRef::Object { slot: you },
            text: "Legend rule".to_owned(),
        },
        LabelView {
            id: 6,
            on: TargetRef::Player {
                player: Side::Opponent,
            },
            text: "Concede".to_owned(),
        },
    ];
    let mut client = client_at(&view);
    let frame = client.frame(1).clone();
    assert!(find(&frame, Role::Button, "Legend rule").is_none());
    assert!(find(&frame, Role::Button, "Concede").is_some());

    press(&mut client, 1, Role::Button, "Move to graveyard");
    assert_eq!(answers(&mut client), [Answer::ApplyLabel { id: 4 }]);
}

#[test]
fn a_permanents_menu_lists_its_abilities_and_x_asks_first() {
    let view = assisted_view();
    let angel = slot_of(&view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let view = with_commands(
        view,
        angel,
        &[("{1}: Gain 1 life.", false), ("−X: Draw X cards.", true)],
        &[],
    );
    let mut client = client_at(&view);
    let rows = menu(&mut client, 1, Role::Card, "Serra Angel");
    assert!(
        rows.iter().any(|row| row == "{1}: Gain 1 life."),
        "{rows:?}"
    );

    press(&mut client, 2, Role::MenuItem, "{1}: Gain 1 life.");
    let [
        Answer::ActivateAbility {
            row: 0, x: None, ..
        },
    ] = answers(&mut client)[..]
    else {
        panic!("the ability row did not activate the first ability");
    };

    menu(&mut client, 3, Role::Card, "Serra Angel");
    press(&mut client, 4, Role::MenuItem, "−X: Draw X cards.");
    assert!(answers(&mut client).is_empty(), "X was not asked for first");
    client.handle(Input::Text(TextChange::Insert("3".to_owned())));
    client.handle(Input::Key(Key {
        code: KeyCode::Enter,
        phase: KeyPhase::Down,
        modifiers: Modifiers::default(),
    }));
    let [
        Answer::ActivateAbility {
            row: 1, x: Some(3), ..
        },
    ] = answers(&mut client)[..]
    else {
        panic!("confirming X did not activate the second ability with X = 3");
    };
}

#[test]
fn abilities_are_not_listed_while_ability_menus_is_off() {
    let mut view = assisted_view();
    view.table.as_mut().unwrap().assists.ability_menus = false;
    let angel = slot_of(&view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let view = with_commands(view, angel, &[("{1}: Gain 1 life.", false)], &[]);
    let mut client = client_at(&view);
    let rows = menu(&mut client, 1, Role::Card, "Serra Angel");
    assert!(
        !rows.iter().any(|row| row.contains("Gain 1 life")),
        "{rows:?}"
    );
}

#[test]
fn a_permanent_that_prints_no_ability_has_no_ability_row() {
    let mut client = client_at(&assisted_view());
    let rows = menu(&mut client, 1, Role::Card, "Serra Angel");
    assert!(
        !rows.iter().any(|row| row == "Activate ability"),
        "{rows:?}"
    );
}

#[test]
fn the_library_menu_scries_after_asking_how_many() {
    let mut client = client_at(&assisted_view());
    let rows = menu(&mut client, 1, Role::Pile, "Your library");
    assert!(rows.iter().any(|row| row == "Scry…"), "{rows:?}");
    assert!(rows.iter().any(|row| row == "Surveil…"), "{rows:?}");

    press(&mut client, 2, Role::MenuItem, "Scry…");
    client.handle(Input::Text(TextChange::Insert("2".to_owned())));
    client.handle(Input::Key(Key {
        code: KeyCode::Enter,
        phase: KeyPhase::Down,
        modifiers: Modifiers::default(),
    }));
    let acts = acts(&mut client);
    assert_eq!(acts.len(), 1, "{acts:?}");
    assert_eq!(acts[0]["flow"]["flow"], "scry");
    assert_eq!(acts[0]["flow"]["count"], 2);
}

#[test]
fn a_cards_menu_offers_the_library_instructions_it_prints() {
    let view = assisted_view();
    let angel = slot_of(&view, ZoneName::Battlefield, Side::You, "Serra Angel");
    let scry = |count| TableFlowOption {
        kind: serde_json::from_value("scry".into()).unwrap(),
        count,
    };
    // The same count printed twice makes one row.
    let view = with_commands(view, angel, &[], &[scry(2), scry(2), scry(1)]);
    let mut client = client_at(&view);
    let rows = menu(&mut client, 1, Role::Card, "Serra Angel");
    let scries: Vec<&String> = rows.iter().filter(|row| row.starts_with("Scry")).collect();
    assert_eq!(scries, ["Scry 2", "Scry 1"]);

    press(&mut client, 2, Role::MenuItem, "Scry 2");
    let acts = acts(&mut client);
    assert_eq!(acts[0]["flow"]["count"], 2);
    assert!(acts[0]["flow"]["source"].is_object(), "{acts:?}");
}

#[test]
fn the_sorting_tray_places_cards_and_finishes_once_all_are_placed() {
    let mut view = assisted_view();
    let library: Vec<ObjectId> = (view.objects.iter())
        .filter(|object| object.zone == ZoneName::Library && object.controller == Side::You)
        .map(|object| object.slot)
        .take(2)
        .collect();
    assert_eq!(library.len(), 2, "the sample library holds two cards");
    let (first, second) = (library[0], library[1]);
    view.table.as_mut().unwrap().prompt = Some(TablePrompt::Sort {
        title: "Scry 2".to_owned(),
        cards: library.clone(),
        piles: vec![PileName::Top, PileName::Bottom],
        placed: vec![vec![second], vec![]],
    });
    let mut client = client_at(&view);
    let frame = client.frame(1).clone();
    assert!(
        find(&frame, Role::Text, "Scry 2").is_some(),
        "no tray title"
    );
    let done = find(&frame, Role::Button, "done").expect("no Done");
    assert!(
        done.state.has(StateBits::DISABLED),
        "Done is enabled with a card unplaced"
    );

    // An unplaced card goes to the end of the top pile, and a placed one to the other pile.
    for slot in [first, second] {
        let at = (frame.resolved.iter())
            .rev()
            .find(|element| element.id.0 == u32::from(slot.0) && element.role == Role::Card)
            .map(|element| element.corners.iter().copied().sum::<Vec2>() / 4.0)
            .expect("the tray shows the card");
        click(&mut client, at, Button::Primary);
    }
    assert_eq!(
        answers(&mut client),
        [
            Answer::Place {
                card: first,
                pile: PileName::Top,
                index: 1
            },
            Answer::Place {
                card: second,
                pile: PileName::Bottom,
                index: 0
            },
        ]
    );
    press(&mut client, 2, Role::Button, "close");
    assert_eq!(answers(&mut client), [Answer::Cancel]);
}
