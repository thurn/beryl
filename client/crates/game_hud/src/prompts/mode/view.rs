//! A choice of how to use a card shows its face once per way, in a row
//! over the table as the web's `Ways` does. Unusable abilities are dimmed.

use beryl_protocol::prompt::{ButtonId, Click, PromptButton, PromptView, ShapeBody};
use beryl_protocol::protocol::{Candidate, CandidateView, ObjectView, Status};
use glam::Vec2;
use kit_base::data::profiles::SlotName;

use super::Mode;
use crate::hud::HudIn;
use crate::hud::placement_scene::{Group, rect};
use crate::prompts::{feature::Details, shade, ways};
use game_table::faces::FaceList;
use kit_base::tokens::{color, ratio, shadow, space, type_style};
use kit_base::typography::TypeStyle;
use kit_frame::frame::Role;
use kit_ui::element::{
    Align, Anchor, Direction, Element, Justify, Shadow, Shown as Showing, TextAlign, row, text,
};
use kit_ui::input::{FaceId, FaceSize};

/// The filter of the face of a way that cannot be chosen, as the web's `.stage-way:disabled`
/// dims and desaturates it (`grayscale(0.85) brightness(0.55)` in `ui/src/stage/choose.css`)
/// rather than making it translucent, which would let the cards behind it show through.
const DIMMED: [kit_ui::element::Filter; 2] = [
    kit_ui::element::Filter::Grayscale(ratio::WAY_DISABLED_GRAYSCALE),
    kit_ui::element::Filter::Brightness(ratio::WAY_DISABLED_BRIGHTNESS),
];

/// The ways stand outside the bar's flow, leaving its buttons in place.
pub(super) fn view(prompt: &PromptView, input: &HudIn, mut bar: Element) -> Element {
    if let Some(row) = ways_row(prompt, input) {
        bar.children.insert(0, shade(input));
        bar.children.push(row);
    }
    bar
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
    let space = input.profile.tray();
    let (tray, per_stage) = space.on_window(input.window.size);
    #[expect(clippy::cast_precision_loss, reason = "a card has a few ways")]
    let count = ways.len() as f32;
    let tray_wide = tray.width / per_stage;
    let gap = space.gap * per_stage;
    // Each way's caption may use the way's whole share of the tray, so that a long one such
    // as "Remote Isle: {2}, Discard this card" wraps to two lines rather than four under a
    // narrow face.
    let share = (tray.width - gap * (count - 1.0)) / count;
    // The face, the gap under it and two lines of caption fit the tray's height, so that a
    // short tray (a phone's) does not push the captions down over the prompt's title.
    let captions = 2.0 * type_style::WAY_CAPTION.size * type_style::WAY_CAPTION.line_height;
    let face_tall = tray.height - (space::M + captions) * per_stage;
    let width = (((tray_wide - space.gap) / count - space.gap).clamp(space.least, space.most)
        * per_stage)
        .min(face_tall * space.card.width / space.card.height)
        .max(0.0);
    let drawn = ways
        .into_iter()
        .zip(way_faces(prompt, input).map(|face| shown_face(face, input.faces)))
        .map(|(way, face)| way_element(way, face, (width, share, space.card), per_stage));
    // Align the choices at their tops so a caption that wraps cannot lift its card.
    let choices = row().gap(gap).align_items(Align::Start).children(drawn);
    // The phone starts below the menu even when captions make the row taller than its tray.
    let vertical = if input.profile.data().table.banded.is_some() {
        Align::Start
    } else {
        Align::Center
    };
    let mut element = row()
        .name("ways")
        .width(tray.width)
        .height(tray.height)
        .justify_content(Justify::Center)
        .align_items(vertical)
        .child(choices);
    let mut top = tray.y;
    if let (Some(scene), Some(slot)) = (
        input.placement_scene,
        input.profile.slot(SlotName::PromptWays),
    ) {
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

fn shown_face(face: Option<FaceId>, faces: Option<&FaceList>) -> Option<Showing> {
    let face = face?;
    let listed = faces?.get(face)?;
    Some(if listed.plate.is_some() {
        Showing::CutoutFace(face, FaceSize::Full)
    } else {
        Showing::Face(face, FaceSize::Full)
    })
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

/// One way to use the card: its face `width` wide, where the face is known, over its label at
/// most `share` wide. A press sends the way's button; a way that cannot be chosen is dimmed and
/// takes no press.
fn way_element(
    way: &PromptButton,
    face: Option<Showing>,
    (width, share, card): (f32, f32, kit_base::data::Card),
    per_stage: f32,
) -> Element {
    let faces = face.map(|face| {
        let element = Element {
            shows: Some(face),
            ..Element::new(Role::Region)
        }
        .name("face")
        .width(width)
        .height(width * card.height / card.width)
        .radius(width * ratio::WAY_FACE_RADIUS)
        .parent_highlight_shadow(&shadow::WAY_HIGHLIGHT);
        if way.enabled {
            element
        } else {
            element.filter(DIMMED.to_vec())
        }
    });
    let caption_shadows = shadow::WAY_CAPTION
        .into_iter()
        .map(|shadow| Shadow {
            y: shadow.y * per_stage,
            blur: shadow.blur * per_stage,
            ..shadow
        })
        .collect();
    let label = text(&way.label)
        .style(TypeStyle {
            size: type_style::WAY_CAPTION.size * per_stage,
            ..type_style::WAY_CAPTION
        })
        .text_align(TextAlign::Center)
        .drop_shadow(caption_shadows)
        .max_width(share.max(width))
        .color(if way.enabled {
            color::TEXT
        } else {
            color::WAY_DISABLED_INK
        });
    let mut element = Element::new(Role::Button);
    element.style.direction = Direction::Column;
    element
        .name(&way.label)
        .background([0; 4])
        .border(([0; 4], 0.0))
        .gap(space::M * per_stage)
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
                    return (object.printing.and_then(|ix| faces.printing(ix)))
                        .or_else(|| faces.land_face(name))
                        .or_else(|| faces.find(name));
                }
                _ => candidate.nth.unwrap_or(0),
            }
        }
        _ => return None,
    };
    faces.choice_plate(name, nth)
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
