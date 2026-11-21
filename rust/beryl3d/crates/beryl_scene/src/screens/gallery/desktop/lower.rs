//! The gallery's lower panels, `04 Cards & states` to `09 Command list`, placed as `desktop`
//! places the upper ones.

use crate::screens::ScreenPress;
use glam::{Vec2, Vec3};

use super::super::{GalleryMenu, GalleryQuery, example_button, face};
use super::{Words, apart, at, caption_at, flat_face, panel, place, rule, words};
use crate::element::{Border, Element, Shadow, Shown, TextAlign, holder};
use crate::faces::{CARD_BACK, FaceList};
use crate::frame::Role;
use crate::input::FaceSize;
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::tokens::{color, shadow, size, type_style};
use crate::widgets::{button, primary};

/// The dim ink of a stepper button that cannot be pressed: `.ask-step:disabled`.
const STEPPER_DISABLED: [u8; 4] = [111, 106, 100, 255];

/// The box of a field card's face on the web, inside its dark rim, when the card stands at
/// `point` `width` points wide.
fn field_face(point: Vec2, width: f32) -> [Vec2; 2] {
    let card = Vec2::new(width, width * CARD_HEIGHT / CARD_WIDTH);
    [point + card * at::FACE_INSET, card * at::FACE_SCALE]
}

/// A field card's face, its corners rounded as the web's frame rounds them.
fn field_card(faces: &FaceList, name: &str, point: Vec2, width: f32) -> Element {
    place(
        flat_face(face(faces, name), name).radius(width * at::FACE_RADIUS),
        field_face(point, width),
    )
}

/// A highlighted field card: a ring round its face, where the web's `highlight` and `combat`
/// light the card.
fn ringed(card: Element, ring: &[Shadow]) -> Element {
    card.box_shadow(ring.to_vec())
}

pub(super) fn card_states(faces: &FaceList) -> Element {
    let [colossus, source, target] = at::STATES_CARDS;
    let width = at::STATES_CARD;
    let mut tapped = flat_face(face(faces, "Mountain"), "Mountain");
    tapped.shows = Some(Shown::TappedFace(face(faces, "Mountain"), FaceSize::Board));
    let hand = ["Exile Lance", "Mountain", "Plateau"]
        .into_iter()
        .zip(at::HAND)
        .zip(at::HAND_WIDTHS.to_array())
        .map(|((name, point), width)| {
            place(
                flat_face(face(faces, name), &format!("{name} in hand")),
                [point, Vec2::new(width, width * CARD_HEIGHT / CARD_WIDTH)],
            )
        });
    panel(3)
        .child(field_card(faces, "Cinder Colossus", colossus, width))
        .child(place(tapped, at::TAPPED))
        .child(place(
            flat_face(face(faces, "Plains"), "Plains"),
            at::UNTAPPED,
        ))
        .child(ringed(
            field_card(faces, "Gloom Wren", source, width),
            &shadow::GALLERY_SOURCE_RING,
        ))
        .child(apart("target").child(ringed(
            field_card(faces, "Gloom Wren", target, width),
            &shadow::GALLERY_TARGET_RING,
        )))
        .children(
            ["Tapped", "Untapped", "Source (gold)", "Target (cyan)"]
                .into_iter()
                .zip(at::STATES_CAPTIONS)
                .map(|(name, point)| caption_at(name, point)),
        )
        .children(hand)
}

pub(super) fn combat(faces: &FaceList) -> Element {
    let [attacker, blocker] = at::COMBATANTS;
    let [colossus, beast] = at::STACK_CARDS;
    let ability = faces
        .plate("Cinder Colossus", true, 0)
        .expect("the Colossus has a trigger");
    panel(4)
        .child(ringed(
            field_card(faces, "Beast", attacker, at::COMBATANT),
            &shadow::GALLERY_ATTACK_RING,
        ))
        .child(ringed(
            field_card(faces, "Angel", blocker, at::COMBATANT),
            &shadow::GALLERY_TARGET_RING,
        ))
        .children(
            ["Attacker · angular", "Blocker · rounded"]
                .into_iter()
                .zip(at::COMBAT_CAPTIONS)
                .map(|(name, point)| caption_at(name, point)),
        )
        .child(rule(at::COMBAT_RULE))
        .child(field_card(
            faces,
            "Cinder Colossus",
            colossus,
            at::STACK_CARD,
        ))
        .child(place(
            flat_face(ability, "Cinder Colossus ability"),
            at::ABILITY,
        ))
        .child(apart("stack").child(field_card(faces, "Beast", beast, at::STACK_CARD)))
}

/// `06 Scry overlay`: the scry overlay over a dimmed table, drawn at 0.35 of its size, as the
/// web's `.scry-preview` scales `ScryOverlay` down into the panel.
pub(super) fn scry(faces: &FaceList) -> Element {
    let label = |name: &str, index: usize| {
        words(
            name,
            type_style::GALLERY_SCRY_LABEL,
            [255; 4],
            [
                at::SCRY_LABELS[index],
                Vec2::new(at::SCRY_LABEL_WIDTHS[index], at::SCRY_LABEL_HEIGHT),
            ],
        )
    };
    // The overlay's buttons keep their full-size radius, which the small pill's height caps.
    let small = |element: Element| {
        element
            .style(type_style::GALLERY_SCRY_BUTTON)
            .radius(size::BUTTON_HEIGHT / 2.0)
            .min_width(0.0)
    };
    let zone = place(
        Element::new(Role::Region)
            .name("drop zone")
            .background([0, 0, 0, 46])
            .border(([242, 236, 225, 115], 2.0))
            .radius(size::GALLERY_SCRY_ZONE_RADIUS),
        at::SCRY_ZONE,
    );
    let backs = at::SCRY_BACKS
        .into_iter()
        .zip(at::SCRY_BACK_SIZES)
        .zip(["first card back", "second card back"])
        .map(|((point, extent), name)| {
            place(
                flat_face(CARD_BACK, name)
                    .background([21, 17, 14, 255])
                    .box_shadow(shadow::GALLERY_PILE_FOOT.to_vec()),
                [point, extent],
            )
        });
    panel(5)
        .child(place(
            holder().name("overlay").background(color::BACKDROP),
            at::SCRY_BACKDROP,
        ))
        .child(words(
            "Scry",
            type_style::GALLERY_SCRY_TITLE,
            [254, 253, 251, 255],
            at::SCRY_TITLE,
        ))
        .child(words(
            "Drag cards to put them on the top or bottom of your library.",
            type_style::GALLERY_SCRY_TEXT,
            [255; 4],
            at::SCRY_INSTRUCTION,
        ))
        .child(zone)
        .children(backs)
        .child(place(
            flat_face(face(faces, "Island"), "Island"),
            at::SCRY_BOTTOM,
        ))
        .child(place(
            flat_face(face(faces, "Quell"), "Quell"),
            at::SCRY_TOP,
        ))
        .child(label("Bottom", 0))
        .child(label("Top", 1))
        .child(place(small(primary("Done")), at::SCRY_DONE))
        .child(place(small(button("View Battlefield")), at::SCRY_VIEW))
        .child(place(
            flat_face(face(faces, "Foresee"), "Foresee"),
            at::SCRY_SOURCE,
        ))
        .children(scry_captions())
}

/// The captions under the scry overlay: its three parts, then the backdrop under them.
fn scry_captions() -> impl Iterator<Item = Element> {
    let widths = [
        at::SCRY_CAPTION_WIDTHS[0].x,
        at::SCRY_CAPTION_WIDTHS[0].y,
        at::SCRY_CAPTION_WIDTHS[1].x,
        at::SCRY_CAPTION_WIDTHS[1].y,
    ];
    [
        "Drag & reorder",
        "Source card",
        "Confirm",
        "Dimmed table backdrop",
    ]
    .into_iter()
    .zip(at::SCRY_CAPTIONS)
    .zip(widths)
    .enumerate()
    .map(|(index, ((name, point), width))| {
        let caption = words(
            name,
            type_style::GALLERY_CAPTION,
            color::TYPE,
            [point, Vec2::new(width, at::CAPTION.y)],
        );
        // A faint line stands before the second and third captions of the row.
        if index == 1 || index == 2 {
            caption.border(
                Border::uniform(color::TYPE, 0.0)
                    .side_color(3, color::GALLERY_CAPTION_RULE)
                    .side_width(3, 1.0),
            )
        } else {
            caption
        }
    })
}

/// `07 Manual table prompts`: the three prompts `AskPrompt` opens, each a column of its cancel
/// button, its field and its confirm button.
pub(super) fn prompts() -> Element {
    let Vec3 {
        x: cancel,
        y: entry,
        z: confirm,
    } = at::PROMPT_ROWS;
    let dock = at::DOCK;
    let [step, gap] = at::STEPPER.to_array();
    let field = |name: &str, value: &str, left: f32, width: f32| {
        place(
            Element::new(Role::Field)
                .name(name)
                .style(type_style::GALLERY_FIELD)
                .color(color::TEXT)
                .text_align(TextAlign::Center)
                .background(color::SURFACE)
                .border((color::GOLD, size::FIELD_BORDER))
                .radius(size::GALLERY_FIELD_RADIUS)
                .words(value),
            [Vec2::new(left, entry), Vec2::new(width, step)],
        )
    };
    // The draw prompt starts at its least value, 1, so its minus is dimmed. It keeps the
    // stepper's own face and rim, as the web's `.ask-step:disabled` only dims its ink.
    let stepper = |name: &str, glyph: &str, left: f32, enabled: bool| {
        place(
            Element::new(Role::Button)
                .name(name)
                .style(type_style::GALLERY_STEPPER)
                .color(if enabled {
                    color::TEXT
                } else {
                    STEPPER_DISABLED
                })
                .text_align(TextAlign::Center)
                .background(color::ENTRY)
                .border((color::ENTRY_RIM, 1.0))
                .radius(step / 2.0)
                .words(glyph),
            [Vec2::new(left, entry), Vec2::splat(step)],
        )
    };
    let docks = [
        ("Cards to draw", "1", "Draw 1"),
        ("Note", "Doesn't untap", "Set note"),
        ("Power and toughness", "3/3", "Set 3/3"),
    ];
    let names = ["Draw cards", "Set note", "Set power and toughness…"];
    let mut panel = panel(6);
    for (index, (name, value, label)) in docks.into_iter().enumerate() {
        let left = at::PROMPT_COLUMNS.to_array()[index];
        // Each prompt is a group of its own, as each `AskPrompt` is a `.gallery-ask` on the web.
        let mut prompt = apart(names[index])
            .child(place(button("Cancel"), [Vec2::new(left, cancel), dock]))
            .child(place(primary(label), [Vec2::new(left, confirm), dock]));
        prompt = if name == "Note" {
            prompt.child(field(name, value, left, dock.x))
        } else {
            prompt
                .child(stepper("Less", "−", left, index != 0))
                .child(field(name, value, left + step + gap, at::STEPPER_FIELD))
                .child(stepper(
                    "More",
                    "+",
                    left + step + gap + at::STEPPER_FIELD + gap,
                    true,
                ))
        };
        panel = panel.child(prompt);
    }
    panel
}

pub(super) fn menus(available: bool) -> Element {
    let samples = [
        ("Permanent", GalleryMenu::Permanent),
        ("Library", GalleryMenu::Library),
        ("Bottom-right corner", GalleryMenu::Corner),
    ];
    let mut panel = panel(7);
    for (index, (label, kind)) in samples.into_iter().enumerate() {
        let width = if kind == GalleryMenu::Corner {
            at::CORNER_WIDTH
        } else {
            size::BUTTON_MIN_WIDTH
        };
        panel = panel.child(place(
            example_button(label, ScreenPress::GalleryMenu(kind), available),
            [
                at::MENU_BUTTONS[index],
                Vec2::new(width, size::BUTTON_HEIGHT),
            ],
        ));
    }
    panel
}

pub(super) fn commands(available: bool) -> Element {
    let samples = [
        ("Empty query", GalleryQuery::Empty),
        ("Two groups", GalleryQuery::Top),
        ("No matches", GalleryQuery::Untap),
    ];
    let mut panel = panel(8);
    for (index, (label, query)) in samples.into_iter().enumerate() {
        panel = panel.child(place(
            example_button(label, ScreenPress::GalleryQuery(Some(query)), available),
            [
                at::COMMAND_BUTTONS[index],
                Vec2::new(size::BUTTON_MIN_WIDTH, size::BUTTON_HEIGHT),
            ],
        ));
    }
    panel
}
