//! The prompt shapes `YesNo`, `Number` and `Mode`: a choice among a few answers, each a button
//! of the prompt bar, as the web interface's `PromptDock` (`ui/src/stage/PromptBar.tsx`) and
//! `Ways` (`ui/src/stage/Choose.tsx`) draw them.
//!
//! - A yes or no, and a choice among answers, holds the cards it is about up in the tray while
//!   it is open (`super::Piles::of`), as the web's `trayOf` does. The yes of a question whose yes
//!   pays a cost shows what it would tap while the pointer is on it, as Auto Pay does.
//! - A number shows its value, with what the value means (the cost an X makes), between a step
//!   down and a step up, each sending the next value the prompt offers that way.
//! - The choice among the ways to use a card (a mode of purpose `act`) draws the card once for
//!   each way, by the face that way plays, and an activated ability as the stack shows it, in a
//!   row over the table where a tray would hold cards up. A press of one sends its button. An
//!   ability the card cannot use now is drawn among them, dimmed, and takes no press.

use beryl_protocol::prompt::{ButtonId, Click, Marked, PromptButton, PromptView, ShapeBody};
use beryl_protocol::protocol::{Candidate, CandidateView, ObjectView, Status};
use beryl_protocol::script::Mark;
use glam::Vec2;

use super::bar::{Controls, bar};
use super::{Shown, Steps, marks, ways};
use crate::faces::FaceList;
use crate::hud::HudIn;
use crate::table::{CARD_HEIGHT, CARD_WIDTH, DESKTOP};
use kit_base::data::space::hud::phone::CHARACTER_WIDTH;
use kit_base::tokens::{color, ratio, size, space, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{
    Align, Anchor, Direction, Element, Justify, Shown as Showing, column, row, text,
};
use kit_ui::input::{FaceId, FaceSize};
use kit_ui::widgets::{icon_button, icons};

/// The filter of the face of a way that cannot be chosen, as the web's `.stage-way:disabled`
/// dims and desaturates it (`grayscale(0.85) brightness(0.55)` in `ui/src/stage/choose.css`)
/// rather than making it translucent, which would let the cards behind it show through.
const DIMMED: [kit_ui::element::Filter; 2] = [
    kit_ui::element::Filter::Grayscale(ratio::WAY_DISABLED_GRAYSCALE),
    kit_ui::element::Filter::Brightness(ratio::WAY_DISABLED_BRIGHTNESS),
];

/// What the flat layer shows for `prompt`: the prompt bar, with a number's steps before its
/// primary button, and, over the table, the ways to use a card where it chooses among them.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    // The ways stand at a point of the window, outside the bar's flow, so the bar is laid out
    // as it is without them.
    Some(bar(prompt, input, None, stepper(prompt)).children(ways_row(prompt, input)))
}

/// Whether `button` of the yes or no `prompt` shows what it would tap: its primary button, the
/// yes, where the question pays a cost and marks what paying it would tap (`suggested`), as the
/// web's `asksToPay` has it.
pub(super) fn shows(prompt: &PromptView, button: ButtonId) -> bool {
    let pays = (prompt.marks.iter()).any(|marked| marked.mark == Mark::Suggested);
    pays && (prompt.buttons.iter()).any(|view| view.id == button && view.primary)
}

/// Begins and ends what the table shows for the prompt `shown`, which was `was`: the tray
/// holds up what it is about, and a question whose yes pays draws what paying would tap only
/// while the pointer is on the yes, as the web stage's `data-pay="asking"` rules in
/// `ui/src/stage/stage.css` have it. Every other mark is drawn as the session sends it, a
/// number's marks on what its value would tap among them.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    marks(shown, was, &look, steps);
}

/// The mark the table draws for `marked` while the prompt is `shown`.
fn look(shown: Shown, marked: Marked) -> Option<Mark> {
    let asking = matches!(shown.prompt.body, ShapeBody::YesNo { .. });
    match marked.mark {
        Mark::Suggested if asking => shown.suggesting.then_some(Mark::Suggested),
        mark => Some(mark),
    }
}

/// A number's value, with what it means, between the step to the value below it and the step
/// to the value above it, as the web's `Stepper` draws it, and about how large it is drawn. A
/// step with no value to go to cannot be pressed. Nothing for any other prompt.
fn stepper(prompt: &PromptView) -> Option<Controls> {
    let ShapeBody::Number {
        values,
        value,
        detail,
    } = &prompt.body
    else {
        return None;
    };
    let at = values.iter().position(|n| n == value);
    let below = at.and_then(|at| at.checked_sub(1)).map(|at| values[at]);
    let above = at.and_then(|at| values.get(at + 1).copied());
    let step = |to: Option<u8>, icon, name: &str| {
        let element = icon_button(icon, name).enabled(to.is_some());
        match to {
            Some(n) => element.on_press(Click::Number { n, on: None }),
            None => element,
        }
    };
    let shown = column()
        .name("value")
        .align_items(Align::Center)
        .child(text(&value.to_string()).style(type_style::TITLE))
        .children(
            (detail.as_deref())
                .map(|detail| text(detail).style(type_style::BODY).color(color::TEXT_DIM)),
        );
    let element = row()
        .name("stepper")
        .gap(space::S)
        .align_items(Align::Center)
        .child(step(below, icons::MINUS, "Less"))
        .child(shown)
        .child(step(above, icons::PLUS, "More"));
    Some(Controls {
        element,
        size: stepper_size(&value.to_string(), detail.as_deref()),
    })
}

/// About how large a number's stepper is drawn with `value` over `detail`, from the count of
/// their characters: an estimate on the generous side, as `bar::button_width` makes, which only
/// decides where the stepper stands on the phone. `detail` is a cost, each `{...}` of which is
/// one mana symbol drawn about as wide as its type is tall, and each other character of which
/// counts as wide as its type is tall too.
fn stepper_size(value: &str, detail: Option<&str>) -> (f32, f32) {
    let line = |style: TypeStyle| style.size * style.line_height;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a value and its cost are a few characters"
    )]
    let (value_width, detail_width) = (
        value.chars().count() as f32 * type_style::TITLE.size * CHARACTER_WIDTH,
        detail.map_or(0.0, |detail| {
            let symbols = detail.split('{').skip(1).count();
            let inside: usize = (detail.split('{').skip(1))
                .map(|part| part.split('}').next().map_or(0, str::len) + 2)
                .sum();
            (detail.chars().count() - inside + symbols) as f32 * type_style::BODY.size
        }),
    );
    let shown = (
        value_width.max(detail_width),
        line(type_style::TITLE) + detail.map_or(0.0, |_| line(type_style::BODY)),
    );
    (
        2.0 * size::ICON_BUTTON + 2.0 * space::S + shown.0,
        size::ICON_BUTTON.max(shown.1),
    )
}

/// The ways to use the card `prompt` is about, in a row over the table where the tray would
/// hold cards up, as the web's `Ways` lays them out: the card once a way, each as wide as the
/// tray's cards are drawn for that many, over what the session calls the way. Nothing where
/// the prompt does not choose among a card's ways, or the view does not show the card.
fn ways_row(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    let ways: Vec<&PromptButton> = ways(prompt).collect();
    if ways.is_empty() {
        return None;
    }
    let faces: Vec<Option<FaceId>> = way_faces(prompt, input).collect();
    let space = DESKTOP.tray_for(input.window);
    let (tray, per_stage) = space.on_window(input.window.size);
    #[expect(clippy::cast_precision_loss, reason = "a card has a few ways")]
    let count = ways.len() as f32;
    let tray_wide = tray.width / per_stage;
    let width =
        ((tray_wide - space.gap) / count - space.gap).clamp(space.least, space.most) * per_stage;
    let drawn = (ways.iter().zip(faces)).map(|(way, face)| way_element(way, face, width));
    let mut element = row()
        .name("ways")
        .width(tray.width)
        .height(tray.height)
        .gap(space.gap * per_stage)
        .justify_content(Justify::Center)
        .align_items(Align::Center)
        .children(drawn);
    element.anchor = Some(Anchor::Point(Vec2::new(tray.x, tray.y), Vec2::ZERO));
    Some(element)
}

/// The face each of the ways to use the card `prompt` is about shows, in the order of its
/// buttons, where the faces and the card are known: none where the prompt does not choose
/// among a card's ways.
pub(super) fn way_faces<'a>(
    prompt: &'a PromptView,
    input: &'a HudIn,
) -> impl Iterator<Item = Option<FaceId>> + 'a {
    let about = match &prompt.body {
        ShapeBody::Mode { about } => about.first(),
        _ => None,
    };
    let object = input
        .view
        .zip(about)
        .and_then(|(view, slot)| view.objects.iter().find(|object| object.slot == *slot));
    let candidates = match input.view.map(|view| &view.status) {
        Some(Status::Deciding { decision }) => &decision.candidates[..],
        _ => &[],
    };
    ways(prompt).map(move |way| {
        let faces = input.faces?;
        way_face(faces, object?, way.id, candidates)
    })
}

/// One way to use the card: its face `width` wide, where the face is known, over its label.
/// A press sends the way's button; a way that cannot be chosen is dimmed and takes no press.
fn way_element(way: &PromptButton, face: Option<FaceId>, width: f32) -> Element {
    let faces = face.map(|face| {
        let element = Element {
            shows: Some(Showing::Face(face, FaceSize::Full)),
            ..Element::new(Role::Region)
        }
        .name("face")
        .width(width)
        .height(width * CARD_HEIGHT / CARD_WIDTH);
        if way.enabled {
            element
        } else {
            element.filter(DIMMED.to_vec())
        }
    });
    let label = text(&way.label)
        .style(type_style::BUTTON)
        .max_width(width)
        .color(if way.enabled {
            color::TEXT
        } else {
            color::TEXT_DIM
        });
    let mut element = Element::new(Role::Button);
    element.style.direction = Direction::Column;
    element
        .name(&way.label)
        .gap(space::M)
        .align_items(Align::Center)
        .children(faces)
        .child(label)
        .enabled(way.enabled)
        .on_press(Click::Button { button: way.id })
}

/// The face the way `id` of using `object` shows, as the web's `Way` draws it: the face a spell
/// is cast with, the face a land is played as, and for an activated ability its plate face, as
/// the stack shows the ability.
fn way_face(
    faces: &FaceList,
    object: &ObjectView,
    id: ButtonId,
    candidates: &[CandidateView],
) -> Option<FaceId> {
    let name = object.name.as_deref()?;
    let nth = match id {
        ButtonId::Ability { nth } => nth,
        ButtonId::Option { index } => {
            let candidate = candidates.get(usize::from(index))?;
            match candidate.option {
                Candidate::Cast { face, .. } => return faces.face_of(name, face),
                Candidate::PlayLand { .. } => {
                    return faces.land_face(name).or_else(|| faces.find(name));
                }
                _ => candidate.nth.unwrap_or(0),
            }
        }
        _ => return None,
    };
    faces.plate(name, false, nth)
}
