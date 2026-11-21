//! The flat layer's first slice: a prompt bar built from a hand-built deciding view, laid out
//! by taffy with text shaped by parley, and the work it costs.

use app_client::Input;
use app_client::command::{Reply, SessionCommand};
use app_client::{Client, Config};
use beryl_protocol::GameUpdate;
use beryl_protocol::prompt::Purpose;
use beryl_protocol::prompt::{
    ButtonId, Click, ClickReply, ClickRequest, PartName, PromptButton, PromptView, ShapeBody, Words,
};
use beryl_protocol::protocol::{
    DecisionId, DecisionView, Kind, SessionId, Side, Status, Why, ZoneName,
};
use beryl_protocol::script::Act;
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder, timing};
use game_hud::prompts::bar::Message as BarMessage;
use game_table::table::Table;
use glam::Vec2;
use kit_base::tokens::{space, type_style};
use kit_frame::frame::{Frame, FrameBuilder, Material, Place, Role, StateBits};
use kit_probe::agent::checks::text_fits;
use kit_probe::agent::tree::{TreeOptions, tree};
use kit_probe::agent::{Context, Rule};
use kit_ui::element::{Action, floats, row, text};
use kit_ui::input::{
    AssetReady, Button, GlyphCell, Insets, Modifiers, Pointer, PointerKind, PointerPhase, Size,
    Tier,
};
use kit_ui::layout::{FloatLook, Layer};

const WIDTH: f32 = 1440.0;
const HEIGHT: f32 = 900.0;
/// A millisecond, in the nanoseconds `Client::frame` takes.
const MS: u64 = 1_000_000;

pub(super) fn client() -> Client {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 256,
            draws: 1024,
            name_bytes: 4096,
        },
    );
    client.handle(Input::Surface {
        size: Size {
            width: WIDTH,
            height: HEIGHT,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client
}

pub(super) fn decision() -> DecisionId {
    DecisionId {
        session: SessionId(1),
        seq: 7,
    }
}

pub(super) fn prompt_button(id: ButtonId, label: &str, primary: bool) -> PromptButton {
    PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled: true,
        primary,
    }
}

/// A mana payment, as the session words one.
pub(super) fn prompt(title: &str) -> PromptView {
    PromptView {
        revision: 3,
        purpose: Purpose::Mana,
        part: PartName::Pick,
        title: title.to_owned(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        previews: Vec::new(),
        buttons: vec![
            prompt_button(ButtonId::Auto, "Auto Pay", true),
            prompt_button(ButtonId::Decline, "Cancel", false),
        ],
        body: ShapeBody::Pay {
            cost: "{1}{U}".to_owned(),
            left: "{1}{U}".to_owned(),
        },
    }
}

/// The sample table, with the viewer deciding `prompt`.
pub(super) fn deciding(prompt: PromptView) -> Input {
    let mut view = ViewBuilder::sample().build();
    view.status = Status::Deciding {
        decision: DecisionView {
            id: decision(),
            kind: Kind::ManaPayment,
            candidates: Vec::new(),
            resolving: None,
            prompt: Box::new(prompt),
            pass_to: None,
            pass_on: None,
            source: None,
            halt: false,
            cancellable: true,
            undo: 0,
            undo_depth: 0,
            refused: Vec::new(),
            acting_for: None,
        },
    };
    Input::Update(Box::new(GameUpdate {
        session: SessionId(1),
        view,
    }))
}

/// Gives every glyph the client wants a made-up place in atlas page 0.
pub(super) fn fill_glyphs(client: &mut Client) {
    let glyphs: Vec<GlyphCell> = client
        .wanted_glyphs()
        .map(|key| GlyphCell {
            key,
            rect: [0, 0, 20, 24],
            bearing: [0, -20],
        })
        .collect();
    client.handle(Input::Assets(AssetReady::Glyphs { page: 0, glyphs }));
}

/// The index in `frame.resolved` of the element of `role` named `name`.
pub(super) fn find(frame: &Frame, role: Role, name: &str) -> usize {
    (frame.resolved.iter())
        .position(|element| element.role == role && frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no {role:?} named {name:?} in the frame"))
}

/// An element's box as (left, top, right, bottom).
pub(super) fn edges(frame: &Frame, index: usize) -> (f32, f32, f32, f32) {
    let corners = frame.resolved[index].corners;
    let min = corners.into_iter().reduce(Vec2::min).unwrap();
    let max = corners.into_iter().reduce(Vec2::max).unwrap();
    (min.x, min.y, max.x, max.y)
}

pub(super) fn centre(frame: &Frame, index: usize) -> Vec2 {
    let (left, top, right, bottom) = edges(frame, index);
    Vec2::new(f32::midpoint(left, right), f32::midpoint(top, bottom))
}

pub(super) fn context() -> Context {
    Context {
        recording: "flat-layer".to_owned(),
        seat: Side::You,
        layout: crate::REGULAR,
        audit: crate::test_data::data().audit,
        checks: crate::REGULAR.checks(
            &crate::test_data::data().profiles,
            kit_base::geometry::Size::default(),
        ),
        size: Size {
            width: WIDTH,
            height: HEIGHT,
        },
        safe_area: Insets::default(),
        at: 0,
    }
}

pub(super) fn pointer(phase: PointerPhase, position: Vec2) -> Input {
    Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })
}

#[test]
fn the_prompt_bar_shows_its_title_and_buttons_in_the_dock() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let opened = client.frame(0);
    assert!(opened.work.layout_nodes > 0 && opened.work.text_runs > 0);
    // Once the bar has faded in, which takes its buttons out of reach for its first frame.
    let frame = client.frame(500 * MS);
    let bar = find(frame, Role::Region, "prompt");
    let title = find(frame, Role::Text, "Pay {1}{U}");
    let auto = find(frame, Role::Button, "Auto Pay");
    let cancel = find(frame, Role::Button, "Cancel");

    // The bar's bottom right corner is where the web interface's action dock stands, and the
    // button the prompt puts forward stands in that corner as the turn button.
    let (bar_left, bar_top, bar_right, bar_bottom) = edges(frame, bar);
    assert!(
        (bar_right - (WIDTH - 39.0)).abs() < 0.01,
        "bar ends at {bar_right}"
    );
    assert!(
        (bar_bottom - (HEIGHT - 87.0)).abs() < 0.01,
        "bar ends at {bar_bottom}"
    );
    let (_, _, auto_right, auto_bottom) = edges(frame, auto);
    assert!((auto_right - bar_right).abs() < 0.01 && (auto_bottom - bar_bottom).abs() < 0.01);
    for element in [title, auto, cancel] {
        assert_eq!(frame.resolved[element].place, Place::Layer);
        let (left, top, right, bottom) = edges(frame, element);
        assert!(left >= bar_left && right <= bar_right && top >= bar_top && bottom <= bar_bottom);
    }
    // The title at the top, the other buttons under it and the turn button at the foot, each
    // button as wide as the dock, which is a turn button's width (`--button-width`).
    assert!(edges(frame, title).3 <= edges(frame, cancel).1);
    assert!(edges(frame, cancel).3 <= edges(frame, auto).1);
    for button in [auto, cancel] {
        let (left, top, right, bottom) = edges(frame, button);
        assert!(
            (right - left - 250.0).abs() < 0.01,
            "a dock button is 250 points wide"
        );
        assert!(
            (bottom - top - 48.0).abs() < 0.01,
            "a button is 48 points tall"
        );
    }
    assert!(frame.resolved[auto].interactive && frame.resolved[cancel].interactive);
    assert!(!frame.resolved[title].interactive);
    assert!(text_fits(frame, &context()).is_empty());
    // Three panels for each button (its rim is lighter on top and darker below than at its
    // sides, a record for each color), the corner's settings gear and bug button, the button of the game log, folded because the opponent's
    // lands reach into its column, and the disc of each of the title's two mana symbols, all in
    // the flat layer's band. The dock draws no panel, as the web's draws none, and neither do
    // the orbs or the life totals on them.
    let layer = &frame.draws[frame.layer as usize..];
    let panels = layer
        .iter()
        .filter(|draw| draw.material == Material::Panel as u8)
        .count();
    assert_eq!(panels, 11);
}

#[test]
fn a_prompt_with_no_title_shows_only_its_buttons() {
    let mut client = client();
    client.handle(deciding(prompt("")));
    let frame = client.frame(0);
    let bar = find(frame, Role::Region, "prompt");
    assert!(
        !frame
            .resolved
            .iter()
            .any(|element| element.role == Role::Text && element.parent == bar as u32),
        "no text for an empty title"
    );
    // The dock holds the two buttons, one over the other with a gap between them, and nothing
    // else.
    let auto = edges(frame, find(frame, Role::Button, "Auto Pay"));
    let cancel = edges(frame, find(frame, Role::Button, "Cancel"));
    let (bar_left, bar_top, bar_right, _) = edges(frame, bar);
    assert!((cancel.1 - bar_top).abs() < 0.01, "{cancel:?} in {bar_top}");
    assert!(
        (auto.1 - cancel.3 - space::M).abs() < 0.01,
        "{auto:?} under {cancel:?}"
    );
    assert!((auto.0 - bar_left).abs() < 0.01 && (bar_right - auto.2).abs() < 0.01);
}

#[test]
fn the_turn_button_has_the_pass_on_button_beside_it_and_its_caption_under_it() {
    let mut client = client();
    let mut view = prompt("");
    view.buttons = vec![
        PromptButton {
            caption: "To Main 2".to_owned(),
            ..prompt_button(ButtonId::Decline, "Next", true)
        },
        prompt_button(ButtonId::PassOn, "To End Turn", false),
    ];
    client.handle(deciding(view));
    let frame = client.frame(500 * MS);
    let bar = edges(frame, find(frame, Role::Region, "prompt"));
    let next = edges(frame, find(frame, Role::Button, "Next"));
    let pass_on = edges(frame, find(frame, Role::Button, "To End Turn"));
    let caption = edges(frame, find(frame, Role::Text, "To Main 2"));
    // As the web's `.turn-action` stands them: neither the round button nor the caption takes
    // room in the dock, so the turn button keeps the dock's corner and width.
    assert!((next.2 - next.0 - 250.0).abs() < 0.01, "{next:?}");
    assert!((next.3 - bar.3).abs() < 0.01 && (next.2 - bar.2).abs() < 0.01);
    assert!(
        (pass_on.2 - (next.0 - space::M)).abs() < 0.01,
        "{pass_on:?} by {next:?}"
    );
    assert!((pass_on.1 - next.1).abs() < 0.01 && (pass_on.3 - pass_on.1 - 48.0).abs() < 0.01);
    assert!(
        (caption.1 - (next.3 + 8.5)).abs() < 0.01,
        "{caption:?} under {next:?}"
    );
    let middle = |(left, _, right, _): (f32, f32, f32, f32)| f32::midpoint(left, right);
    assert!(
        (middle(caption) - middle(next)).abs() < 0.5,
        "{caption:?} under {next:?}"
    );
}

#[test]
fn glyphs_are_asked_for_once_and_drawn_when_a_page_holds_them() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let glyph_count = |frame: &Frame| {
        (frame.draws.iter())
            .filter(|draw| draw.material == Material::Glyph as u8)
            .count()
    };
    let before = client.frame(0);
    assert_eq!(glyph_count(before), 0, "a missing glyph draws nothing");
    let places: Vec<(usize, [Vec2; 4])> = (before.resolved.iter().enumerate())
        .map(|(index, element)| (index, element.corners))
        .collect();
    fill_glyphs(&mut client);
    assert_eq!(
        client.wanted_glyphs().count(),
        0,
        "each glyph is asked for once"
    );
    let after = client.frame(1);
    // "Pay" and a glyph on each of its two mana symbols, "Auto Pay" and "Cancel".
    assert!(glyph_count(after) >= 3 + 2 + 7 + 6);
    let moved = (after.resolved.iter().enumerate())
        .any(|(index, element)| places[index] != (index, element.corners));
    assert!(!moved, "a glyph arriving moves nothing");
}

#[test]
fn an_equal_view_lays_out_and_shapes_nothing() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let opened = client.frame(0);
    assert!(opened.work.layout_nodes > 0 && opened.work.text_runs > 0);
    // Once the bar has faded in.
    let first = client.frame(500 * MS).clone();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let second = client.frame(500 * MS + 1);
    assert_eq!(second.work.layout_nodes, 0);
    assert_eq!(second.work.text_runs, 0);
    assert_eq!(second.resolved, first.resolved);
    let idle = client.frame(500 * MS + 2);
    assert_eq!((idle.work.layout_nodes, idle.work.text_runs), (0, 0));
}

#[test]
fn a_new_title_shapes_only_the_title() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    client.handle(deciding(prompt("Pay {2}{U}")));
    let frame = client.frame(1);
    assert_eq!(frame.work.text_runs, 1);
    find(frame, Role::Text, "Pay {2}{U}");
}

#[test]
fn a_title_too_long_for_its_box_fails_text_fits() {
    let mut layer = Layer::new();
    layer.show(Some(
        row().name("prompt").child(
            text("Choose any number of target creatures your opponents control")
                .style(type_style::TITLE)
                .width(120.0)
                .height(30.0),
        ),
    ));
    let mut frame = Frame::with_capacity(16, 16, 1024);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    layer.write(
        &mut builder,
        &Table::new(
            crate::test_data::data(),
            game_table::table::Window::default(),
        )
        .camera()
        .screen_plane(),
        true,
    );
    let frame = builder.finish();
    let faults = text_fits(frame, &context());
    assert!(!faults.is_empty());
    assert!(faults.iter().all(|fault| fault.rule == Rule::TextFits));
    assert!(
        faults[0].message.contains("Choose any number"),
        "{}",
        faults[0].message
    );
}

#[test]
fn pressing_cancel_queues_the_click() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let at = centre(frame, find(frame, Role::Button, "Cancel"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    assert_eq!(
        client.next_command(),
        Some(SessionCommand::Click(ClickRequest {
            decision: decision(),
            revision: 3,
            click: Click::Button {
                button: ButtonId::Decline
            },
        }))
    );
    assert_eq!(client.next_command(), None);
}

#[test]
fn a_disabled_button_queues_nothing() {
    let mut client = client();
    let mut view = prompt("Pay {1}{U}");
    view.buttons[1].enabled = false;
    client.handle(deciding(view));
    let frame = client.frame(0);
    let cancel = find(frame, Role::Button, "Cancel");
    assert!(frame.resolved[cancel].state.has(StateBits::DISABLED));
    let at = centre(frame, cancel);
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    assert_eq!(client.next_command(), None);
}

#[test]
fn a_click_reply_replaces_the_prompt() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    let mut next = prompt("Pay {U}");
    next.revision = 4;
    client.handle(Input::Reply(Reply::Click(Ok(ClickReply::Open {
        prompt: Box::new(next),
    }))));
    let frame = client.frame(1);
    let cancel = centre(frame, find(frame, Role::Button, "Cancel"));
    find(frame, Role::Text, "Pay {U}");
    client.handle(pointer(PointerPhase::Down, cancel));
    client.handle(pointer(PointerPhase::Up, cancel));
    let Some(SessionCommand::Click(click)) = client.next_command() else {
        panic!("the press queues a click");
    };
    assert_eq!(click.revision, 4);
}

#[test]
fn an_answer_to_the_decision_before_leaves_the_next_prompt_open() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let cancel = centre(frame, find(frame, Role::Button, "Cancel"));
    client.handle(pointer(PointerPhase::Down, cancel));
    client.handle(pointer(PointerPhase::Up, cancel));
    assert!(matches!(
        client.next_command(),
        Some(SessionCommand::Click(_))
    ));
    // The game thread sends the next decision's view before the session thread's answer to
    // the click that ended the decision before it.
    let mut next = deciding(prompt("Pay {U}"));
    if let Input::Update(update) = &mut next
        && let Status::Deciding { decision } = &mut update.view.status
    {
        decision.id.seq += 1;
    }
    client.handle(next);
    client.handle(Input::Reply(Reply::Click(Ok(ClickReply::Answered))));
    let frame = client.frame(1);
    find(frame, Role::Text, "Pay {U}");
    // A click on the visible next prompt must use its id and current revision.
    let at = centre(frame, find(frame, Role::Button, "Auto Pay"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    let Some(SessionCommand::Click(sent)) = client.next_command() else {
        panic!("the next prompt still takes clicks");
    };
    assert_eq!(sent.decision.seq, decision().seq + 1);
    assert_eq!(sent.revision, 3);
}

#[test]
fn two_clients_given_the_same_inputs_return_equal_frames() {
    let run = || {
        let mut client = client();
        client.handle(deciding(prompt("Pay {1}{U}")));
        fill_glyphs(&mut client);
        client.frame(5).clone()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_prompt_opens_only_once_its_view_has_played_out() {
    let mut client = client();
    client.handle(Input::Update(Box::new(ViewBuilder::sample().update())));
    client.frame(0);
    // The next view asks for a payment and arrives with a script in which the viewer draws the
    // top card of their library, a flight of 400 ms.
    let Input::Update(mut update) = deciding(prompt("Pay {1}{U}")) else {
        unreachable!("deciding gives an update");
    };
    let view = &mut update.view;
    let top = view
        .players
        .you
        .library
        .pop()
        .expect("the sample has a library");
    let card = view
        .objects
        .iter_mut()
        .find(|object| object.slot == top)
        .expect("the library card is an object");
    card.zone = ZoneName::Hand;
    card.name = Some("Island".to_owned());
    let mut script = ScriptBuilder::default()
        .beat([Act::Fly {
            from: ZoneName::Library,
            why: Why::Draw,
            object: card.clone(),
            at: None,
            item: None,
            origin: None,
        }])
        .build();
    script.timing = vec![timing(400.0)];
    view.script = Some(script);
    client.handle(Input::Update(update));

    // While the card flies, the bar is neither drawn nor pressed. The rest of the screen stays
    // up, as the web interface's `Chrome` does: the players' orbs and the log's button show.
    let frame = client.frame(MS);
    let named = |name: &str| {
        (frame.resolved.iter())
            .any(|element| element.place == Place::Layer && frame.name(element.name) == name)
    };
    assert!(
        !named("Cancel") && !named("Pay {1}{U}"),
        "no prompt bar while the draw plays"
    );
    assert!(
        named("your orb") && named("Show log"),
        "the orbs and the log stay up while the draw plays"
    );
    let open = {
        let mut later = self::client();
        later.handle(deciding(prompt("Pay {1}{U}")));
        let frame = later.frame(0);
        centre(frame, find(frame, Role::Button, "Cancel"))
    };
    client.handle(pointer(PointerPhase::Down, open));
    client.handle(pointer(PointerPhase::Up, open));
    client.frame(200 * MS);
    assert_eq!(
        client.next_command(),
        None,
        "a hidden button takes no press"
    );

    // Once the card has landed, the bar is drawn and its buttons take presses.
    let frame = client.frame(500 * MS);
    let at = centre(frame, find(frame, Role::Button, "Cancel"));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    assert!(
        matches!(client.next_command(), Some(SessionCommand::Click(_))),
        "the open bar's Cancel queues a click"
    );
}

/// The float every float test shows: a life total of 20.
const LIFE: kit_ui::element::FloatId = floats::LIFE[0];

/// A look standing a float's centre at `point`.
fn centred_at(point: Vec2) -> FloatLook {
    FloatLook {
        point,
        anchor: Vec2::new(0.5, 0.5),
        ..FloatLook::default()
    }
}

/// `layer` written into a new frame, with what waits for the table (`Element::waits`) drawn if
/// `open`, and the layout work done since the last frame.
pub(super) fn write_layer(layer: &mut Layer, open: bool) -> Frame {
    let mut frame = Frame::with_capacity(64, 256, 1024);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    let (layout_nodes, text_runs) = layer.take_work();
    builder.layout_work(layout_nodes, text_runs);
    layer.write(
        &mut builder,
        &Table::new(
            crate::test_data::data(),
            game_table::table::Window::default(),
        )
        .camera()
        .screen_plane(),
        open,
    );
    builder.finish().clone()
}

#[test]
fn a_float_is_centred_on_its_point() {
    let mut layer = Layer::new();
    layer.float(LIFE, Some(text("20")));
    layer.set_float_look(LIFE, centred_at(Vec2::new(100.0, 50.0)));
    let frame = write_layer(&mut layer, false);
    let life = find(&frame, Role::Text, "20");
    let at = centre(&frame, life);
    assert!(
        at.distance(Vec2::new(100.0, 50.0)) < 0.01,
        "centred at {at}"
    );
    assert_eq!(frame.resolved[life].place, Place::Layer);
    // The tree printout, as `Page::tree` prints it, shows the float under the flat layer.
    let tree = tree(&frame, &TreeOptions::default()).expect("the whole tree");
    let (left, top, right, bottom) = edges(&frame, life);
    let expected = format!(
        "text \"20\" #{} {},{} {}x{}",
        frame.resolved[life].id.0,
        left.round(),
        top.round(),
        (right - left).round(),
        (bottom - top).round()
    );
    assert!(tree.contains("  layer\n"), "{tree}");
    assert!(tree.contains(&expected), "{expected:?} in {tree}");
}

#[test]
fn a_new_look_moves_a_float_and_lays_out_nothing() {
    let mut layer = Layer::new();
    layer.float(LIFE, Some(text("20")));
    layer.set_float_look(LIFE, centred_at(Vec2::new(100.0, 50.0)));
    let before = write_layer(&mut layer, false);
    assert!(before.work.layout_nodes > 0);
    layer.set_float_look(LIFE, centred_at(Vec2::new(300.0, 80.0)));
    let after = write_layer(&mut layer, false);
    assert_eq!(after.work.layout_nodes, 0);
    assert_eq!(after.work.text_runs, 0);
    let moved = |frame: &Frame| frame.resolved[find(frame, Role::Text, "20")].corners;
    for (old, new) in moved(&before).into_iter().zip(moved(&after)) {
        assert!(
            (new - old).distance(Vec2::new(200.0, 30.0)) < 0.01,
            "{old} to {new}"
        );
    }
}

#[test]
fn a_removed_float_leaves_the_frame() {
    let mut layer = Layer::new();
    layer.float(LIFE, Some(text("20")));
    write_layer(&mut layer, false);
    layer.float(LIFE, None);
    let frame = write_layer(&mut layer, false);
    assert!(frame.resolved.is_empty(), "{:?}", frame.resolved);
    assert_eq!(frame.draws.len(), 0);
}

#[test]
fn a_float_leaves_the_prompt_bar_where_it_was() {
    let mut layer = Layer::new();
    layer.show(Some(
        row()
            .name("prompt")
            .padding(space::L)
            .child(text("Pay {1}{U}").style(type_style::TITLE)),
    ));
    let alone = write_layer(&mut layer, true);
    layer.float(LIFE, Some(text("20").padding(space::L)));
    layer.set_float_look(LIFE, centred_at(Vec2::new(WIDTH / 2.0, HEIGHT / 2.0)));
    let beside = write_layer(&mut layer, true);
    for name in ["prompt", "Pay {1}{U}"] {
        let role = if name == "prompt" {
            Role::Region
        } else {
            Role::Text
        };
        assert_eq!(
            alone.resolved[find(&alone, role, name)].corners,
            beside.resolved[find(&beside, role, name)].corners,
            "{name} moved"
        );
    }
    find(&beside, Role::Text, "20");
}

#[test]
fn a_float_is_drawn_while_no_prompt_is_open() {
    let mut layer = Layer::new();
    layer.show(Some(
        row()
            .name("screen")
            .child(row().name("prompt").child(text("Pay {1}{U}")).waits()),
    ));
    layer.float(LIFE, Some(text("20")));
    layer.set_float_look(
        LIFE,
        FloatLook {
            opacity: 0.5,
            tint: Some([255, 0, 0, 255]),
            ..centred_at(Vec2::new(100.0, 50.0))
        },
    );
    let frame = write_layer(&mut layer, false);
    let life = find(&frame, Role::Text, "20");
    assert!((frame.resolved[life].opacity - 0.5).abs() < 1e-6);
    assert!(
        !frame
            .resolved
            .iter()
            .any(|element| frame.name(element.name) == "prompt"),
        "the closed prompt bar is not drawn"
    );
    find(&frame, Role::Region, "screen");
    // The float's top element is drawn on a panel, faded by its look. The screen's top element,
    // drawn first, has a panel of its own at full alpha.
    let layer_draws = &frame.draws[frame.layer as usize..];
    let panels: Vec<_> = (layer_draws.iter())
        .filter(|draw| draw.material == Material::Panel as u8)
        .collect();
    assert_eq!(panels.len(), 2);
    assert_eq!(panels[1].tint[3], 0x70, "half of the panel's alpha");
}

#[test]
fn a_float_with_a_press_takes_presses_and_one_without_does_not() {
    let mut layer = Layer::new();
    layer.float(LIFE, Some(text("20")));
    layer.set_float_look(LIFE, centred_at(Vec2::new(100.0, 50.0)));
    let at = Vec2::new(100.0, 50.0);
    assert_eq!(layer.press_at(at, false), None);
    layer.float(
        LIFE,
        Some(text("20").on_press(BarMessage::Button(ButtonId::Auto))),
    );
    let frame = write_layer(&mut layer, false);
    let id = frame.resolved[find(&frame, Role::Text, "20")].id;
    assert_eq!(
        layer.press_at(at, false),
        Some((id, Action::new(BarMessage::Button(ButtonId::Auto)), true)),
        "a kept float keeps its look and takes presses with no prompt open"
    );
}
