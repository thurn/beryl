//! A choice of how to use a card shows its face once per way, in a row
//! over the table as the web's `Ways` does. Unusable abilities are dimmed.

use beryl_protocol::prompt::{ButtonId, Click, PromptButton, PromptView, ShapeBody};
use beryl_protocol::protocol::{Candidate, CandidateView, ObjectView, Status};
use glam::Vec2;

use super::Mode;
use crate::hud::HudIn;
use crate::hud::placement_scene::{Group, rect};
use crate::prompts::{feature::Details, ways};
use game_table::faces::FaceList;
use game_table::table::{CARD_HEIGHT, CARD_WIDTH};
use kit_base::tokens::{color, ratio, space, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Anchor, Direction, Element, Justify, Shown as Showing, row, text};
use kit_ui::input::{FaceId, FaceSize};

/// The filter of the face of a way that cannot be chosen, as the web's `.stage-way:disabled`
/// dims and desaturates it (`grayscale(0.85) brightness(0.55)` in `ui/src/stage/choose.css`)
/// rather than making it translucent, which would let the cards behind it show through.
const DIMMED: [kit_ui::element::Filter; 2] = [
    kit_ui::element::Filter::Grayscale(ratio::WAY_DISABLED_GRAYSCALE),
    kit_ui::element::Filter::Brightness(ratio::WAY_DISABLED_BRIGHTNESS),
];

/// The ways stand outside the bar's flow, leaving its buttons in place.
pub(super) fn view(prompt: &PromptView, input: &HudIn, bar: Element) -> Element {
    bar.children(ways_row(prompt, input))
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
    let space = input.profile.tray();
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
    let mut top = tray.y;
    if let (Some(scene), Some(slot)) = (input.placement_scene, input.profile.slot("prompt.ways")) {
        let area = slot.rect;
        let above = area.y + area.height - tray.height;
        let candidates = [top, above]
            .into_iter()
            .filter(|&top| top >= area.y)
            .map(|top| rect(tray.x, top, tray.width, tray.height).into());
        // The choices are shown over a shaded table. Prefer the normal tray, then the
        // last position above the viewer's front row before using the shaded tray.
        let avoids = vec![Group::YourRows.children(), Group::Stack.children()];
        if let Some((_, placed)) = scene.place("ways to use a card", avoids, candidates) {
            top = placed.y;
        }
    }
    element.anchor = Some(Anchor::Point(Vec2::new(tray.x, top), Vec2::ZERO));
    Some(element)
}

/// The face each of the ways to use the card `prompt` is about shows, in the order of its
/// buttons, where the faces and the card are known: none where the prompt does not choose
/// among a card's ways.
pub(in crate::prompts) fn way_faces<'a>(
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
        .on_press(Mode::message(Click::Button { button: way.id }))
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

pub(super) fn details(input: &HudIn<'_>) -> Details {
    Details::Ways(
        input
            .prompt
            .into_iter()
            .flat_map(|prompt| way_faces(prompt, input))
            .collect(),
    )
}
