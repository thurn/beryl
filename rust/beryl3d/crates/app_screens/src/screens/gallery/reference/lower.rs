//! Places the gallery's lower panels, `04 Cards & states` to `09 Command list`,
//! using the selected profile's reference layout.

use crate::screens::ScreenPress;
use glam::Vec2;

use super::super::{GalleryMenu, GalleryQuery, example_button, face};
use super::{Words, apart, caption_at, flat_face, panel, place, rule, words};
use game_table::faces::{CARD_BACK, FaceList};
use kit_base::data::Card;
use kit_base::data::profiles::GalleryLayout;
use kit_base::tokens::{color, shadow, size, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Border, Element, Shadow, Shown, TextAlign, holder};
use kit_ui::input::FaceSize;
use kit_ui::widgets::{button, primary};

/// The dim ink of a stepper button that cannot be pressed: `.ask-step:disabled`.
const STEPPER_DISABLED: [u8; 4] = [111, 106, 100, 255];

/// The box of a field card's face on the web, inside its dark rim, when the card stands at
/// `point` `width` points wide.
fn field_face(layout: &GalleryLayout, point: Vec2, (width, shape): (f32, Card)) -> [Vec2; 2] {
    let card = Vec2::new(width, width * shape.height / shape.width);
    [point + card * layout.face_inset, card * layout.face_scale]
}

/// A field card's face, its corners rounded as the web's frame rounds them.
fn field_card(
    layout: &GalleryLayout,
    (faces, shape): (&FaceList, Card),
    name: &str,
    point: Vec2,
    width: f32,
) -> Element {
    place(
        flat_face(face(faces, name), name).radius(width * layout.face_radius),
        field_face(layout, point, (width, shape)),
    )
}

/// A highlighted field card: a ring round its face, where the web's `highlight` and `combat`
/// light the card.
fn ringed(card: Element, ring: &[Shadow]) -> Element {
    card.box_shadow(ring.to_vec())
}

pub(super) fn card_states(layout: &GalleryLayout, (faces, shape): (&FaceList, Card)) -> Element {
    let [colossus, source, target] = layout.states_cards;
    let width = layout.states_card;
    let mut tapped = flat_face(face(faces, "Mountain"), "Mountain");
    tapped.shows = Some(Shown::TappedFace(face(faces, "Mountain"), FaceSize::Board));
    let hand = ["Exile Lance", "Mountain", "Plateau"]
        .into_iter()
        .zip(layout.hand)
        .zip(layout.hand_widths)
        .map(|((name, point), width)| {
            place(
                flat_face(face(faces, name), &format!("{name} in hand")),
                [point, Vec2::new(width, width * shape.height / shape.width)],
            )
        });
    panel(layout, 3)
        .child(field_card(
            layout,
            (faces, shape),
            "Cinder Colossus",
            colossus,
            width,
        ))
        .child(place(tapped, layout.tapped))
        .child(place(
            flat_face(face(faces, "Plains"), "Plains"),
            layout.untapped,
        ))
        .child(ringed(
            field_card(layout, (faces, shape), "Gloom Wren", source, width),
            &shadow::GALLERY_SOURCE_RING,
        ))
        .child(apart("target").child(ringed(
            field_card(layout, (faces, shape), "Gloom Wren", target, width),
            &shadow::GALLERY_TARGET_RING,
        )))
        .children(
            ["Tapped", "Untapped", "Source (gold)", "Target (cyan)"]
                .into_iter()
                .zip(layout.states_captions)
                .map(|(name, point)| caption_at(layout, name, point)),
        )
        .children(hand)
}

pub(super) fn combat(layout: &GalleryLayout, (faces, shape): (&FaceList, Card)) -> Element {
    let [attacker, blocker] = layout.combatants;
    let [colossus, beast] = layout.stack_cards;
    let ability = faces
        .plate("Cinder Colossus", true, 0)
        .expect("the Colossus has a trigger");
    panel(layout, 4)
        .child(ringed(
            field_card(layout, (faces, shape), "Beast", attacker, layout.combatant),
            &shadow::GALLERY_ATTACK_RING,
        ))
        .child(ringed(
            field_card(layout, (faces, shape), "Angel", blocker, layout.combatant),
            &shadow::GALLERY_TARGET_RING,
        ))
        .children(
            ["Attacker · angular", "Blocker · rounded"]
                .into_iter()
                .zip(layout.combat_captions)
                .map(|(name, point)| caption_at(layout, name, point)),
        )
        .child(rule(layout.combat_rule))
        .child(field_card(
            layout,
            (faces, shape),
            "Cinder Colossus",
            colossus,
            layout.stack_card,
        ))
        .child(place(
            flat_face(ability, "Cinder Colossus ability"),
            layout.ability,
        ))
        .child(apart("stack").child(field_card(
            layout,
            (faces, shape),
            "Beast",
            beast,
            layout.stack_card,
        )))
}

/// `06 Scry overlay`: the scry overlay over a dimmed table, drawn at 0.35 of its size, as the
/// web's `.scry-preview` scales `ScryOverlay` down into the panel.
pub(super) fn scry(layout: &GalleryLayout, faces: &FaceList) -> Element {
    let label = |name: &str, index: usize| {
        words(
            name,
            type_style::GALLERY_SCRY_LABEL,
            [255; 4],
            [
                layout.scry_labels[index],
                Vec2::new(layout.scry_label_widths[index], layout.scry_label_height),
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
        layout.scry_zone,
    );
    let backs = layout
        .scry_backs
        .into_iter()
        .zip(layout.scry_back_sizes)
        .zip(["first card back", "second card back"])
        .map(|((point, extent), name)| {
            place(
                flat_face(CARD_BACK, name)
                    .background([21, 17, 14, 255])
                    .box_shadow(shadow::GALLERY_PILE_FOOT.to_vec()),
                [point, extent],
            )
        });
    panel(layout, 5)
        .child(place(
            holder().name("overlay").background(color::BACKDROP),
            layout.scry_backdrop,
        ))
        .child(words(
            "Scry",
            type_style::GALLERY_SCRY_TITLE,
            [254, 253, 251, 255],
            layout.scry_title,
        ))
        .child(words(
            "Drag cards to put them on the top or bottom of your library.",
            type_style::GALLERY_SCRY_TEXT,
            [255; 4],
            layout.scry_instruction,
        ))
        .child(zone)
        .children(backs)
        .child(place(
            flat_face(face(faces, "Island"), "Island"),
            layout.scry_bottom,
        ))
        .child(place(
            flat_face(face(faces, "Quell"), "Quell"),
            layout.scry_top,
        ))
        .child(label("Bottom", 0))
        .child(label("Top", 1))
        .child(place(small(primary("Done")), layout.scry_done))
        .child(place(small(button("View Battlefield")), layout.scry_view))
        .child(place(
            flat_face(face(faces, "Foresee"), "Foresee"),
            layout.scry_source,
        ))
        .children(scry_captions(layout))
}

/// The captions under the scry overlay: its three parts, then the backdrop under them.
fn scry_captions(layout: &GalleryLayout) -> impl Iterator<Item = Element> {
    let widths = [
        layout.scry_caption_widths[0].x,
        layout.scry_caption_widths[0].y,
        layout.scry_caption_widths[1].x,
        layout.scry_caption_widths[1].y,
    ];
    [
        "Drag & reorder",
        "Source card",
        "Confirm",
        "Dimmed table backdrop",
    ]
    .into_iter()
    .zip(layout.scry_captions)
    .zip(widths)
    .enumerate()
    .map(move |(index, ((name, point), width))| {
        let caption = words(
            name,
            type_style::GALLERY_CAPTION,
            color::TYPE,
            [point, Vec2::new(width, layout.caption.y)],
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
pub(super) fn prompts(layout: &GalleryLayout) -> Element {
    let [cancel, entry, confirm] = layout.prompt_rows;
    let dock = layout.dock;
    let [step, gap] = layout.stepper.to_array();
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
    let mut panel = panel(layout, 6);
    for (index, (name, value, label)) in docks.into_iter().enumerate() {
        let left = layout.prompt_columns[index];
        // Each prompt is a group of its own, as each `AskPrompt` is a `.gallery-ask` on the web.
        let mut prompt = apart(names[index])
            .child(place(button("Cancel"), [Vec2::new(left, cancel), dock]))
            .child(place(primary(label), [Vec2::new(left, confirm), dock]));
        prompt = if name == "Note" {
            prompt.child(field(name, value, left, dock.x))
        } else {
            prompt
                .child(stepper("Less", "−", left, index != 0))
                .child(field(name, value, left + step + gap, layout.stepper_field))
                .child(stepper(
                    "More",
                    "+",
                    left + step + gap + layout.stepper_field + gap,
                    true,
                ))
        };
        panel = panel.child(prompt);
    }
    panel
}

pub(super) fn menus(layout: &GalleryLayout, available: bool) -> Element {
    let samples = [
        ("Permanent", GalleryMenu::Permanent),
        ("Library", GalleryMenu::Library),
        ("Bottom-right corner", GalleryMenu::Corner),
    ];
    let mut panel = panel(layout, 7);
    for (index, (label, kind)) in samples.into_iter().enumerate() {
        let width = if kind == GalleryMenu::Corner {
            layout.corner_width
        } else {
            size::BUTTON_MIN_WIDTH
        };
        panel = panel.child(place(
            example_button(label, ScreenPress::GalleryMenu(kind), available),
            [
                layout.menu_buttons[index],
                Vec2::new(width, size::BUTTON_HEIGHT),
            ],
        ));
    }
    panel
}

pub(super) fn commands(layout: &GalleryLayout, available: bool) -> Element {
    let samples = [
        ("Empty query", GalleryQuery::Empty),
        ("Two groups", GalleryQuery::Top),
        ("No matches", GalleryQuery::Untap),
    ];
    let mut panel = panel(layout, 8);
    for (index, (label, query)) in samples.into_iter().enumerate() {
        panel = panel.child(place(
            example_button(label, ScreenPress::GalleryQuery(Some(query)), available),
            [
                layout.command_buttons[index],
                Vec2::new(size::BUTTON_MIN_WIDTH, size::BUTTON_HEIGHT),
            ],
        ));
    }
    panel
}
