//! The flat layer's first slice: a prompt bar built from a hand-built deciding view, laid out
//! by taffy with text shaped by parley, and the work it costs.

use beryl_engine::card::Purpose;
use beryl_scene::agent::checks::text_fits;
use beryl_scene::agent::{Context, Layout, Rule};
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::element::{row, text};
use beryl_scene::frame::{Frame, FrameBuilder, Material, Place, Role, StateBits};
use beryl_scene::input::{
    AssetReady, Button, GlyphCell, Input, Insets, Modifiers, Pointer, PointerKind, PointerPhase,
    Size,
};
use beryl_scene::layout::Layer;
use beryl_scene::table::Table;
use beryl_scene::testing::ViewBuilder;
use beryl_scene::text::Text;
use beryl_scene::text::fonts::Font;
use beryl_scene::tokens::{space, type_style};
use beryl_scene::{Client, Config};
use beryl_session::GameUpdate;
use beryl_session::prompt::{
    ButtonId, Click, ClickReply, ClickRequest, PartName, PromptButton, PromptView, ShapeBody, Words,
};
use beryl_session::protocol::{DecisionId, DecisionView, Kind, SessionId, Side, Status};
use glam::Vec2;

const WIDTH: f32 = 1440.0;
const HEIGHT: f32 = 900.0;

fn client() -> Client {
    let mut client = Client::new(Config {
        elements: 256,
        draws: 1024,
        name_bytes: 4096,
    });
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

fn decision() -> DecisionId {
    DecisionId {
        session: SessionId(1),
        seq: 7,
    }
}

fn prompt_button(id: ButtonId, label: &str, primary: bool) -> PromptButton {
    PromptButton {
        id,
        label: label.to_owned(),
        caption: String::new(),
        enabled: true,
        primary,
    }
}

/// A mana payment, as the session words one.
fn prompt(title: &str) -> PromptView {
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
fn deciding(prompt: PromptView) -> Input {
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
        },
    };
    Input::Update(Box::new(GameUpdate {
        session: SessionId(1),
        view,
    }))
}

/// Gives every glyph the client wants a made-up place in atlas page 0.
fn fill_glyphs(client: &mut Client) {
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
fn find(frame: &Frame, role: Role, name: &str) -> usize {
    (frame.resolved.iter())
        .position(|element| element.role == role && frame.name(element.name) == name)
        .unwrap_or_else(|| panic!("no {role:?} named {name:?} in the frame"))
}

/// An element's box as (left, top, right, bottom).
fn edges(frame: &Frame, index: usize) -> (f32, f32, f32, f32) {
    let corners = frame.resolved[index].corners;
    let min = corners.into_iter().reduce(Vec2::min).unwrap();
    let max = corners.into_iter().reduce(Vec2::max).unwrap();
    (min.x, min.y, max.x, max.y)
}

fn centre(frame: &Frame, index: usize) -> Vec2 {
    let (left, top, right, bottom) = edges(frame, index);
    Vec2::new(f32::midpoint(left, right), f32::midpoint(top, bottom))
}

fn context() -> Context {
    Context {
        recording: "flat-layer".to_owned(),
        seat: Side::You,
        layout: Layout::Desktop,
        size: Size {
            width: WIDTH,
            height: HEIGHT,
        },
        safe_area: Insets::default(),
        at: 0,
    }
}

fn pointer(phase: PointerPhase, position: Vec2) -> Input {
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
    let frame = client.frame(0);
    let bar = find(frame, Role::Region, "prompt");
    let title = find(frame, Role::Text, "Pay {1}{U}");
    let auto = find(frame, Role::Button, "Auto Pay");
    let cancel = find(frame, Role::Button, "Cancel");

    // The bar's bottom right corner is where the web interface's action dock stands.
    let (bar_left, bar_top, bar_right, bar_bottom) = edges(frame, bar);
    assert!(
        (bar_right - (WIDTH - 39.0)).abs() < 0.01,
        "bar ends at {bar_right}"
    );
    assert!(
        (bar_bottom - (HEIGHT - 87.0)).abs() < 0.01,
        "bar ends at {bar_bottom}"
    );
    for element in [title, auto, cancel] {
        assert_eq!(frame.resolved[element].parent, bar as u32);
        assert_eq!(frame.resolved[element].place, Place::Layer);
        let (left, top, right, bottom) = edges(frame, element);
        assert!(left >= bar_left && right <= bar_right && top >= bar_top && bottom <= bar_bottom);
    }
    // The title, then the buttons in the order the session sent them, left to right.
    assert!(edges(frame, title).2 <= edges(frame, auto).0);
    assert!(edges(frame, auto).2 <= edges(frame, cancel).0);
    let (_, top, _, bottom) = edges(frame, cancel);
    assert!(
        (bottom - top - 48.0).abs() < 0.01,
        "a button is 48 points tall"
    );
    assert!(frame.resolved[auto].interactive && frame.resolved[cancel].interactive);
    assert!(!frame.resolved[title].interactive);
    assert!(text_fits(frame, &context()).is_empty());
    // A panel for the bar and one for each button, all in the flat layer's band.
    let layer = &frame.draws[frame.layer as usize..];
    let panels = layer
        .iter()
        .filter(|draw| draw.material == Material::Panel as u8)
        .count();
    assert_eq!(panels, 3);
    assert!(frame.work.layout_nodes > 0 && frame.work.text_runs > 0);
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
            .any(|element| element.role == Role::Text),
        "no text for an empty title"
    );
    // The bar holds its padding, the two buttons and one gap between them, and nothing else.
    let auto = edges(frame, find(frame, Role::Button, "Auto Pay"));
    let cancel = edges(frame, find(frame, Role::Button, "Cancel"));
    let (bar_left, _, bar_right, _) = edges(frame, bar);
    let padding = space::L;
    assert!(
        (auto.0 - bar_left - padding).abs() < 0.01,
        "{auto:?} in {bar_left}"
    );
    assert!(
        (bar_right - cancel.2 - padding).abs() < 0.01,
        "{cancel:?} in {bar_right}"
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
    // "Pay {1}{U}" without its spaces, "Auto Pay" and "Cancel".
    assert!(glyph_count(after) >= 8 + 7 + 6);
    let moved = (after.resolved.iter().enumerate())
        .any(|(index, element)| places[index] != (index, element.corners));
    assert!(!moved, "a glyph arriving moves nothing");
}

#[test]
fn an_equal_view_lays_out_and_shapes_nothing() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let first = client.frame(0).clone();
    assert!(first.work.layout_nodes > 0 && first.work.text_runs > 0);
    client.handle(deciding(prompt("Pay {1}{U}")));
    let second = client.frame(1);
    assert_eq!(second.work.layout_nodes, 0);
    assert_eq!(second.work.text_runs, 0);
    assert_eq!(second.resolved, first.resolved);
    let idle = client.frame(2);
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
    let mut builder = FrameBuilder::new(&mut frame);
    layer.write(&mut builder, Table::default().camera());
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
fn a_variable_font_is_drawn_at_any_weight() {
    let mut text = Text::new();
    let regular = beryl_scene::text::TypeStyle {
        font: Font::OpenSans,
        weight: 400,
        size: 20.0,
        line_height: 1.2,
    };
    let bold = beryl_scene::text::TypeStyle {
        weight: 700,
        ..regular
    };
    let (regular_width, _) = text.measure("Counterspell", regular, None);
    let (bold_width, _) = text.measure("Counterspell", bold, None);
    assert!(
        bold_width > regular_width + 1.0,
        "{bold_width} vs {regular_width}"
    );
}
