//! The game screen's furniture: the step stops about each player's orb, and the commands a
//! press on one sends; and the game log, which folds to a button when a permanent reaches into
//! its column.

use app_client::Input;
use app_client::command::{Request, SessionCommand};
use beryl_protocol::StopsSetting;
use beryl_protocol::protocol::{CardType, GameEvent, SessionId, Side, Step, ZoneName};
use beryl_protocol::testing::ViewBuilder;
use kit_frame::frame::{Frame, Role, StateBits};
use kit_ui::input::PointerPhase;

use beryl_protocol::prompt::{ButtonId, Click, ClickRequest, Purpose, ShapeBody};
use beryl_protocol::protocol::{Kind, Status};
use glam::Vec2;
use kit_base::paint::Filter;
use kit_base::tokens::gradient;
use kit_frame::frame::GradientEntry;

use super::flat_layer::{centre, client, deciding, edges, find, pointer, prompt, prompt_button};

/// Whether `frame` has an element of `role` named `name`.
fn has(frame: &Frame, role: Role, name: &str) -> bool {
    (frame.resolved.iter()).any(|element| element.role == role && frame.name(element.name) == name)
}

#[test]
fn pressing_the_opponents_draw_step_sets_that_stop_and_draws_it_filled() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let draw = find(frame, Role::Toggle, "Opponent's draw");
    assert!(!frame.resolved[draw].state.has(StateBits::SELECTED));
    let at = centre(frame, draw);
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));

    let mut want = StopsSetting::SMART;
    want.theirs = beryl_protocol::protocol::StepSet::of(&[
        Step::DeclareAttackers.into(),
        Step::DeclareBlockers.into(),
        Step::End.into(),
        Step::Draw.into(),
    ]);
    assert_eq!(
        client.next_command(),
        Some(SessionCommand::SetStops(SessionId(1), want))
    );
    let saved = std::iter::from_fn(|| client.next_request()).find_map(|request| match request {
        Request::SaveSettings(settings) => Some(settings.stops),
        _ => None,
    });
    assert_eq!(saved, Some(want));

    let frame = client.frame(1_000_000);
    let draw = find(frame, Role::Toggle, "Opponent's draw");
    assert!(frame.resolved[draw].state.has(StateBits::SELECTED));
    // The viewer's own draw step is a stop of its own, and stays unset.
    let yours = find(frame, Role::Toggle, "Your draw");
    assert!(!frame.resolved[yours].state.has(StateBits::SELECTED));
}

#[test]
fn the_damage_icon_sets_both_combat_damage_steps() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let at = centre(frame, find(frame, Role::Toggle, "Your damage"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    let Some(SessionCommand::SetStops(_, stops)) = client.next_command() else {
        panic!("no stops sent");
    };
    assert!(stops.yours.has(Step::FirstStrikeDamage.into()));
    assert!(stops.yours.has(Step::CombatDamage.into()));
}

/// The sample table while the opponent acts: the viewer has no decision open.
fn waiting() -> Input {
    Input::Update(Box::new(ViewBuilder::sample().update()))
}

/// Presses the toggle named `name`, as the last frame drew it.
fn press_toggle(client: &mut app_client::Client, now: u64, name: &str) {
    let frame = client.frame(now);
    let at = centre(frame, find(frame, Role::Toggle, name));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
}

/// The stops commands the client has queued.
fn stops_sent(client: &mut app_client::Client) -> Vec<StopsSetting> {
    std::iter::from_fn(|| client.next_command())
        .filter_map(|command| match command {
            SessionCommand::SetStops(_, stops) => Some(stops),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stop_set_while_the_opponent_acts_is_sent_at_the_next_decision() {
    // The session takes stops only at the viewer's decision, and refuses them otherwise.
    let mut client = client();
    client.handle(waiting());
    press_toggle(&mut client, 0, "Opponent's draw");
    assert_eq!(stops_sent(&mut client), []);

    client.handle(deciding(prompt("Pay {1}{U}")));
    let sent = stops_sent(&mut client);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].theirs.has(Step::Draw.into()));
    // Frames between updates do not resend the command.
    client.frame(1_000_000);
    assert_eq!(stops_sent(&mut client), []);
}

#[test]
fn a_new_game_takes_up_the_saved_stops_at_its_first_decision() {
    let mut client = client();
    let mut settings = app_client::settings::Settings::default();
    settings.stops.full_control = true;
    client.handle(Input::Settings(settings));
    assert_eq!(stops_sent(&mut client), []);
    client.handle(deciding(prompt("Pay {1}{U}")));
    let sent = stops_sent(&mut client);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].full_control);
}

#[test]
fn a_stop_set_and_cleared_in_one_decision_sends_both_changes() {
    // The latest view still holds the stops from before the first press, so the second press,
    // which matches them, must still be sent: the game holds the first press's.
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    press_toggle(&mut client, 0, "Opponent's draw");
    press_toggle(&mut client, 1_000_000, "Opponent's draw");
    assert_eq!(
        stops_sent(&mut client).last(),
        Some(&StopsSetting::SMART),
        "the game is told the stop is cleared again"
    );
}

#[test]
fn an_earlier_stop_acknowledgement_keeps_the_later_change_pending() {
    let decision = || deciding(prompt("Pay {1}{U}"));
    let mut client = client();
    client.handle(decision());
    press_toggle(&mut client, 0, "Opponent's draw");
    press_toggle(&mut client, 1_000_000, "Opponent's draw");
    let sent = stops_sent(&mut client);
    assert_eq!(sent.len(), 2);

    for stops in sent {
        let Input::Update(mut update) = decision() else {
            unreachable!("a decision is sent in a game update");
        };
        update.view.stops = stops;
        client.handle(Input::Update(update));
        assert_eq!(stops_sent(&mut client), []);
    }
}

#[test]
fn untap_and_cleanup_show_an_icon_but_no_toggle() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    for whose in ["Your", "Opponent's"] {
        for step in ["untap", "cleanup"] {
            let name = format!("{whose} {step}");
            assert!(has(frame, Role::Icon, &name), "{name} has an icon");
            assert!(!has(frame, Role::Toggle, &name), "{name} is no toggle");
        }
    }
    // The other ten steps of each player are toggles.
    let toggles = (frame.resolved.iter())
        .filter(|element| element.role == Role::Toggle)
        .count();
    assert_eq!(toggles, 20);
}

#[test]
fn phase_stop_hover_fades_the_icon_and_places_the_label_on_each_side() {
    let mut client = client();
    client.set_looking(true);
    client.handle(deciding(prompt("Pay {1}{U}")));

    for (start, side, name, step) in [
        (0, Side::You, "Your damage", Step::CombatDamage),
        (1_000, Side::Opponent, "Opponent's draw", Step::Draw),
    ] {
        let frame = client.frame(start * 1_000_000);
        let toggle = find(frame, Role::Toggle, name);
        let rest = frame.resolved[find(frame, Role::Icon, name)].opacity;
        let at = centre(frame, toggle);
        let (_, icon_top, _, icon_bottom) = edges(frame, toggle);
        client.handle(pointer(PointerPhase::Move, at));
        client.frame((start + 1) * 1_000_000);
        let frame = client.frame((start + 61) * 1_000_000);
        let fading = frame.resolved[find(frame, Role::Icon, name)].opacity;
        assert!(rest < fading && fading < 1.0);
        let tip = find(frame, Role::Text, &format!("tooltip {name}"));
        assert_eq!(tooltip_words(frame, tip), name);
        assert!(!frame.resolved[tip].interactive);
        let (left, top, right, bottom) = edges(frame, tip);
        assert!((f32::midpoint(left, right) - at.x).abs() < 0.01);
        let gap = match side {
            Side::You => icon_top - bottom,
            Side::Opponent => top - icon_bottom,
        };
        assert!((gap - 8.0).abs() < 0.01);

        let frame = client.frame((start + 121) * 1_000_000);
        assert!((frame.resolved[find(frame, Role::Icon, name)].opacity - 1.0).abs() < 0.01);
        assert_eq!(stops_sent(&mut client), []);
        press_toggle(&mut client, (start + 122) * 1_000_000, name);
        let mut want = StopsSetting::SMART;
        let set = match side {
            Side::You => &mut want.yours,
            Side::Opponent => &mut want.theirs,
        };
        *set = game_hud::hud::steps::toggled(*set, step);
        assert_eq!(stops_sent(&mut client), [want]);
        press_toggle(&mut client, (start + 123) * 1_000_000, name);
        assert_eq!(stops_sent(&mut client), [StopsSetting::SMART]);

        client.handle(pointer(PointerPhase::Move, glam::Vec2::splat(5.0)));
        let frame = client.frame((start + 124) * 1_000_000);
        assert!(!has(frame, Role::Text, &format!("tooltip {name}")));
        let frame = client.frame((start + 244) * 1_000_000);
        assert!((frame.resolved[find(frame, Role::Icon, name)].opacity - rest).abs() < 0.01);
    }
}

#[test]
fn phase_hover_names_the_current_turn_and_leaves_fixed_icons_dim() {
    let mut client = client();
    client.set_looking(true);
    let Input::Update(mut update) = deciding(prompt("Pay {1}{U}")) else {
        unreachable!("a decision is sent in a game update");
    };
    update.view.turn.number = 8;
    update.view.turn.own_number = 4;
    update.view.turn.active = Side::You;
    update.view.turn.step = Step::FirstStrikeDamage;
    client.handle(Input::Update(update));

    let frame = client.frame(0);
    let at = centre(frame, find(frame, Role::Toggle, "Your damage"));
    client.handle(pointer(PointerPhase::Move, at));
    let frame = client.frame(1_000_000);
    let tip = find(frame, Role::Text, "tooltip Your damage");
    assert_eq!(tooltip_words(frame, tip), "Turn 4 · First-strike damage");

    let fixed = find(frame, Role::Icon, "Your cleanup");
    let rest = frame.resolved[fixed].opacity;
    let at = centre(frame, fixed);
    client.handle(pointer(PointerPhase::Move, at));
    client.frame(2_000_000);
    let frame = client.frame(122_000_000);
    let tip = find(frame, Role::Text, "tooltip Your cleanup");
    assert_eq!(tooltip_words(frame, tip), "Your cleanup");
    assert!((frame.resolved[find(frame, Role::Icon, "Your cleanup")].opacity - rest).abs() < 0.01);
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    assert_eq!(stops_sent(&mut client), []);
}

fn tooltip_words(frame: &Frame, tip: usize) -> &str {
    let tip = u32::try_from(tip).expect("a frame index fits in u32");
    let line = frame
        .lines
        .iter()
        .find(|line| line.element == tip)
        .expect("a tooltip has text");
    frame.name(line.words)
}

/// A view of `lands` lands on each side of the table, besides a creature each, whose events say
/// the viewer gained 2 life.
fn lands(lands: usize) -> Input {
    let mut builder = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        for _ in 0..lands {
            builder.card(ZoneName::Battlefield, side, "Plains").types = vec![CardType::Land];
        }
        builder
            .card(ZoneName::Battlefield, side, "Serra Angel")
            .types = vec![CardType::Creature];
    }
    let mut update = builder.update();
    update.view.events = vec![GameEvent::LifeGain {
        player: Side::You,
        amount: 2,
    }];
    Input::Update(Box::new(update))
}

#[test]
fn the_log_is_a_button_until_it_is_opened() {
    let mut client = client();
    client.handle(lands(0));
    let frame = client.frame(0);
    assert!(!has(frame, Role::List, "Game log"), "the log starts open");
    let at = centre(frame, find(frame, Role::Button, "Show log"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    let frame = client.frame(100_000_000);
    assert!(has(frame, Role::Button, "Hide log"));
    assert!(has(frame, Role::Text, "You gained 2 life"));
}

// Generic story checks do not toggle the game log open or assert non-overlap between the log list
// and the shared library across every desktop window size (K20).
#[test]
fn column_log_panel_clears_the_shared_library_across_window_sizes() {
    use beryl_protocol::GameUpdate;
    use beryl_protocol::protocol::{Facing, SharedPiles};
    use kit_ui::input::{Input as DeviceInput, Insets, Size};

    let mut builder = ViewBuilder::new();
    let hidden = builder.card(ZoneName::Library, Side::You, "Dandan").slot;
    let top = builder.card(ZoneName::Library, Side::You, "Island").slot;
    let mut view = builder.build();
    for object in &mut view.objects {
        object.facing = Facing::Down;
        object.name = None;
        object.zone_side = None;
    }
    view.players.you.library.clear();
    view.players.opponent.library.clear();
    view.shared = Some(SharedPiles {
        library: vec![hidden, top],
        graveyard: Vec::new(),
    });
    view.events = vec![GameEvent::LifeGain {
        player: Side::You,
        amount: 2,
    }];

    for (width, height) in [(1440.0, 900.0), (1600.0, 900.0), (1920.0, 1080.0)] {
        let mut client = client();
        client.handle(Input::Device(DeviceInput::Surface {
            size: Size { width, height },
            scale: 1.0,
            safe_area: Insets::default(),
        }));
        client.handle(Input::Update(Box::new(GameUpdate {
            session: SessionId(1),
            view: view.clone(),
        })));
        let frame = client.frame(0);
        let at = centre(frame, find(frame, Role::Button, "Show log"));
        client.handle(pointer(PointerPhase::Down, at));
        client.handle(pointer(PointerPhase::Up, at));

        let frame = client.frame(100_000_000);
        let log = edges(frame, find(frame, Role::List, "Game log"));
        let library = edges(frame, find(frame, Role::Pile, "Library"));
        let overlaps =
            log.0 < library.2 && log.2 > library.0 && log.1 < library.3 && log.3 > library.1;
        assert!(
            !overlaps,
            "at {width}x{height}: log {log:?} overlapped library {library:?}"
        );
    }
}

#[test]
fn the_recording_driver_reports_a_features_live_state() {
    use app_client::driver::{Page, Recording};
    use beryl_protocol::ipc::FixtureMessage;
    use kit_ui::input::Size;

    let Input::Update(update) = lands(0) else {
        unreachable!("lands are sent in a view update");
    };
    let recording = Recording(vec![FixtureMessage::game_update(&update)]);
    let mut page = Page::new(
        crate::test_data::data(),
        recording.into(),
        Side::You,
        Size {
            width: 1440.0,
            height: 900.0,
        },
    )
    .expect("the view can be read");
    assert!(page.feature_state("unknown").is_none());
    let closed = page.feature_state("game-log").expect("registered game log");
    page.press("button \"Show log\"");
    let opened = page.feature_state("game-log").expect("registered game log");
    assert_ne!(closed, opened);
    page.press("button \"Hide log\"");
    assert_eq!(page.feature_state("game-log"), Some(closed));
}

#[test]
fn pass_on_brightens_only_while_hovered_and_still_sends_its_action() {
    for primary in ["Resolve", "Next"] {
        let mut client = client();
        let prompt = beryl_protocol::testing::prompt(
            Purpose::Priority,
            "",
            vec![
                prompt_button(ButtonId::Decline, primary, true),
                prompt_button(ButtonId::PassOn, "To End", false),
            ],
            ShapeBody::Pick {
                least: 0,
                most: 1,
                picked: Vec::new(),
                tray: Vec::new(),
                warned: Vec::new(),
            },
        );
        let Input::Update(mut update) = deciding(prompt) else {
            unreachable!("deciding is an update");
        };
        let Status::Deciding { decision } = &mut update.view.status else {
            unreachable!("the view has a decision");
        };
        decision.kind = Kind::Priority;
        if primary == "Next" {
            update.view.stack.clear();
        } else {
            update.view.turn.active = Side::Opponent;
            let spell = update.view.objects.last_mut().expect("sample spell");
            spell.controller = Side::Opponent;
            spell.owner = Side::Opponent;
        }
        let decision = decision.id;
        client.handle(Input::Update(update));

        let bright = gradient::PRIMARY.map_colors(|color| Filter::Brightness(1.1).color(color));
        let bright = GradientEntry::of(&bright.resolve(Vec2::splat(48.0)));
        let paints_bright = |frame: &Frame| {
            frame.paints.table.gradients[..usize::from(frame.paints.gradients)].contains(&bright)
        };
        let frame = client.frame(0);
        let at = centre(frame, find(frame, Role::Button, "To End"));
        assert!(!paints_bright(frame), "{primary}: resting face");

        client.handle(pointer(PointerPhase::Move, at));
        assert!(
            paints_bright(client.frame(1_000_000)),
            "{primary}: hovered face"
        );
        client.handle(pointer(PointerPhase::Move, Vec2::new(700.0, 400.0)));
        assert!(
            !paints_bright(client.frame(2_000_000)),
            "{primary}: pointer left"
        );

        client.handle(pointer(PointerPhase::Move, at));
        client.handle(pointer(PointerPhase::Down, at));
        client.handle(pointer(PointerPhase::Up, at));
        assert!(matches!(
            client.next_command(),
            Some(SessionCommand::Click(ClickRequest {
                decision: sent,
                click: Click::Button { button: ButtonId::PassOn },
                ..
            })) if sent == decision
        ));
    }
}

/// A deciding view whose prompt has a turn button and, if `pass_on`, the round button that passes
/// on, with full control on in the stops the game holds.
fn full_control_on(pass_on: bool) -> Input {
    let mut view = prompt("");
    view.buttons = vec![prompt_button(ButtonId::Decline, "Next", true)];
    if pass_on {
        view.buttons
            .push(prompt_button(ButtonId::PassOn, "To End Turn", false));
    }
    let Input::Update(mut update) = deciding(view) else {
        unreachable!("deciding is an update")
    };
    update.view.stops.full_control = true;
    Input::Update(update)
}

#[test]
fn full_controls_sign_stands_left_of_the_turn_button_and_moves_over_for_pass_on() {
    let mut client = client();
    let frame = client.frame(0);
    assert!(
        !has(frame, Role::Icon, "Full control"),
        "no sign before a game"
    );
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(1_000_000);
    assert!(
        !has(frame, Role::Icon, "Full control"),
        "no sign while full control is off"
    );

    // As the web's `.full-control` stands it: 12 points left of the turn button, centred on it.
    client.handle(full_control_on(false));
    let frame = client.frame(2_000_000);
    let next = edges(frame, find(frame, Role::Button, "Next"));
    let sign = edges(frame, find(frame, Role::Icon, "Full control"));
    assert!(
        (next.0 - sign.2 - 12.0).abs() < 0.01,
        "{sign:?} by {next:?}"
    );
    assert!(
        (sign.1 - next.1 - 10.0).abs() < 0.01,
        "{sign:?} by {next:?}"
    );

    // With the round pass-on button left of the turn button, the sign stands 12 points left of
    // that instead.
    client.handle(full_control_on(true));
    let frame = client.frame(3_000_000);
    let pass_on = edges(frame, find(frame, Role::Button, "To End Turn"));
    let sign = edges(frame, find(frame, Role::Icon, "Full control"));
    assert!(
        (pass_on.0 - sign.2 - 12.0).abs() < 0.01,
        "{sign:?} by {pass_on:?}"
    );
}

/// The placement of the spinner's dim arc in `frame`, which is the only icon drawn in its color
/// and strength.
fn tail(frame: &Frame) -> Vec<[f32; 12]> {
    let [r, g, b, _] = kit_base::tokens::color::THINKING;
    (frame.draws.iter())
        .filter(|draw| draw.tint == [r, g, b, 110])
        .map(|draw| draw.model)
        .collect()
}

#[test]
fn ai_thinking_indicator_shows_while_thinking_and_hides_otherwise() {
    let mut client = client();
    let cells = game_hud::art::atlas_cells(&client.data().profiles)
        .enumerate()
        .map(|(index, layer)| layer.cell([u16::try_from(index).unwrap(), 0, 1, 1]))
        .collect();
    client.handle(Input::Device(kit_ui::input::Input::Assets(
        kit_ui::input::AssetReady::Icons { page: 1, cells },
    )));

    // While the AI opponent is thinking, a spinner shows under the opponent's orb.
    let mut thinking = ViewBuilder::sample().update();
    thinking.view.status = Status::Thinking;
    thinking.view.turn.active = Side::Opponent;
    client.handle(Input::Update(Box::new(thinking.clone())));

    let frame = client.frame(1_000_000);
    let ring = find(frame, Role::Region, "Opponent thinking");
    let orb = find(frame, Role::Region, "opponent's orb");
    assert!(
        centre(frame, ring).y > centre(frame, orb).y,
        "thinking spinner should sit below opponent's orb"
    );
    let start = tail(frame);
    assert!(!start.is_empty(), "the spinner's arc should be drawn");
    assert!(
        matches!(client.display_wake(), kit_frame::frame::Wake::Now),
        "live display should keep turning while thinking"
    );
    assert!(
        !matches!(client.wake(), kit_frame::frame::Wake::Now),
        "the spinner must not delay play"
    );
    // A quarter of a turn later the arc has turned, and a whole turn later it is back.
    let quarter = tail(client.frame(1_000_000 + 200_000_000));
    assert_ne!(start, quarter, "the spinner's arc should turn over time");
    let whole = tail(client.frame(1_000_000 + 800_000_000));
    assert_eq!(start.len(), whole.len());
    for (a, b) in start.iter().zip(&whole) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-2, "one turn takes 0.8 s: {a:?} vs {b:?}");
        }
    }

    // When the AI acts (Status::Playing), the indicator disappears immediately.
    client.handle(waiting());
    let frame = client.frame(1_000_000 + 800_000_000);
    assert!(!has(frame, Role::Region, "Opponent thinking"));

    // While the player is deciding, the indicator does not show.
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(1_000_000 + 900_000_000);
    assert!(!has(frame, Role::Region, "Opponent thinking"));

    // At a manual table (Status::Free), the indicator does not show.
    let mut free = ViewBuilder::sample().update();
    free.view.status = Status::Free;
    client.handle(Input::Update(Box::new(free)));
    let frame = client.frame(1_000_000 + 1_000_000_000);
    assert!(!has(frame, Role::Region, "Opponent thinking"));

    // On a compact layout, the indicator also shows while thinking and is scaled smaller.
    client.handle(Input::Device(kit_ui::input::Input::Surface {
        size: kit_ui::input::Size {
            width: 393.0,
            height: 759.0,
        },
        scale: 2.0,
        safe_area: kit_ui::input::Insets::default(),
    }));
    client.handle(Input::Update(Box::new(thinking)));
    let compact_frame = client.frame(1_000_000 + 1_100_000_000);
    let compact_ring = find(compact_frame, Role::Region, "Opponent thinking");
    let compact_edges = edges(compact_frame, compact_ring);
    let compact_width = compact_edges.2 - compact_edges.0;
    assert!(
        compact_width < 25.0,
        "compact thinking spinner should scale with orb_scale, got width {compact_width}"
    );
}
