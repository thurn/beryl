//! The desktop gallery, laid out as the web's component library page (`ui/src/screens/Gallery.tsx`
//! and `ui/src/screens/styles/gallery.css`, drawn after `docs/ui_mockups/ui_system.png`): a page
//! 1536 points wide, centred in the window, with the wordmark and subtitle at its top over nine
//! numbered panels. The first six stand three to a row and the last three span the page.
//!
//! Every part stands where the web puts it. A panel is at its `.panel-0n` rule's place on the
//! page, and each sample in a panel is at the web's `At` point or in its `CaptionAt` box, in
//! points from the inside of the panel's border, as CSS places an absolute child in its parent's
//! padding box. Those points, and the boxes the web's text and samples take, are
//! supplied by the selected profile, under `gallery`.

mod lower;

use beryl_protocol::protocol::Side;
use glam::Vec2;

use super::face;
use game_hud::art::orb::artwork;
use game_table::faces::{CARD_BACK, FaceList};
use kit_base::data::profiles::GalleryLayout;
use kit_base::tokens::{color, gradient, shadow, size, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{Border, Element, Overflow, Position, Shown, TextAlign, holder, text};
use kit_ui::input::{FaceId, FaceSize};
use kit_ui::widgets::{button, icons, primary};

/// The numbered panels' titles, in order.
const TITLES: [&str; 9] = [
    "Foundations",
    "Actions & status",
    "Player & zones",
    "Cards & states",
    "Combat & stack",
    "Scry overlay",
    "Manual table prompts",
    "Context menus",
    "Command list",
];

/// A panel's border, lit from above: `.panel`'s `border-color` sides with `border-top-color:
/// #e9c88a` and `border-bottom-color: #a9895f`.
const PANEL_RIM_TOP: [u8; 4] = [0xe9, 0xc8, 0x8a, 0xff];
const PANEL_RIM_BOTTOM: [u8; 4] = [0xa9, 0x89, 0x5f, 0xff];

/// The color swatches of `01 Foundations`, each with its chip's rim: a cream rim 1.5 points wide
/// round the two dark ones, and a light tint of its own color 1 point wide round the others
/// (`.swatch-chip`'s `color-mix`).
const SWATCHES: [(&str, &str, [u8; 4], [u8; 4]); 5] = [
    ("Table", "#6B5F56", color::TABLE, [0xe4, 0xdc, 0xcf, 0xff]),
    (
        "Surface",
        "#211F1C",
        color::SURFACE,
        [0xe4, 0xdc, 0xcf, 0xff],
    ),
    ("Text", "#F2E7D8", color::TEXT, [245, 236, 224, 255]),
    ("Gold", "#E4A242", color::GOLD, [240, 212, 171, 255]),
    ("Cyan", "#33DCFE", color::CYAN, [178, 232, 237, 255]),
];

/// The page, as wide and tall as the web's `.gallery`, with every panel on it.
pub(super) fn page(layout: &GalleryLayout, faces: &FaceList, available: bool) -> Element {
    holder()
        .name("page")
        .width(layout.page.x)
        .height(layout.page.y)
        .shrink(0.0)
        .child(place(
            text("PROJECT BERYL")
                .name("Project Beryl")
                .style(type_style::GALLERY_WORDMARK)
                .color(color::GALLERY_WORDMARK)
                .text_align(TextAlign::Center),
            layout.wordmark,
        ))
        .children(layout.wordmark_rules.map(|point| {
            place(
                holder().background_image(vec![
                    gradient::GALLERY_WORDMARK_SHADE,
                    gradient::GALLERY_WORDMARK_RULE,
                ]),
                [point, layout.wordmark_rule],
            )
        }))
        .child(place(
            text("Game UI · Component Library")
                .style(type_style::GALLERY_SUBTITLE)
                .color(color::TEXT)
                .text_align(TextAlign::Center),
            layout.subtitle,
        ))
        .child(foundations(layout))
        .child(actions(layout))
        .child(player_zones(layout, faces))
        .child(lower::card_states(layout, faces))
        .child(lower::combat(layout, faces))
        .child(lower::scry(layout, faces))
        .child(lower::prompts(layout))
        .child(lower::menus(layout, available))
        .child(lower::commands(layout, available))
}

/// Gives an element `words` to show, laid out inside its padding.
trait Words {
    fn words(self, words: &str) -> Self;
}

impl Words for Element {
    fn words(mut self, words: &str) -> Self {
        self.text = Some(words.to_owned());
        self
    }
}

/// Keeps a swatch's name to the first line of its caption's box, as the web's `figcaption`
/// sets the name on its first line and the hex value under it.
trait FirstLine {
    fn first_line(self, bottom_padding: f32) -> Self;
}

impl FirstLine for Element {
    fn first_line(mut self, bottom_padding: f32) -> Self {
        self.style.padding[2] = bottom_padding;
        self
    }
}

/// `element` at `area`'s point in its parent's padding box, at `area`'s size.
fn place(element: Element, [point, extent]: [Vec2; 2]) -> Element {
    element
        .position(Position::Absolute)
        .inset([Some(point.y), None, None, Some(point.x)])
        .width(extent.x)
        .height(extent.y)
        .shrink(0.0)
}

/// Panel `index` (0 for `01`): its box on the page, its border lit from above and its numbered
/// title, over the engraved rule that `.panel-title::after` draws (left out here).
fn panel(layout: &GalleryLayout, index: usize) -> Element {
    let extent = layout.panels_size[index];
    // The scry panel's title has no inset: `.panel-scry` drops the panels' side padding.
    let inset = if index == 5 { 0.0 } else { layout.title_inset };
    let number = format!("{:02}", index + 1);
    // The second panel's title starts 4 points further left: `.panel-02 .panel-title` sets a
    // left padding of 12 points where the other panels' titles have 16.
    let pull = if index == 1 {
        layout.second_title_pull
    } else {
        0.0
    };
    let mut number_at = layout.title_number;
    number_at[0].x -= pull;
    // As on the web, the title's own words follow its number, a gold text of its own.
    let mut title = text(TITLES[index])
        .name(&format!("{number} {}", TITLES[index]))
        .style(type_style::GALLERY_TITLE)
        .color(color::TEXT)
        .child(place(
            // Beryl 3D's digits are wider than the web's, so the number shrinks into the web's
            // box.
            text(&number)
                .style(type_style::GALLERY_TITLE_NUMBER)
                .color(color::GOLD)
                .fit(true),
            number_at,
        ));
    title.style.padding = [
        layout.title_padding.y,
        0.0,
        0.0,
        layout.title_padding.x - pull,
    ];
    Element::new(Role::Region)
        .name(TITLES[index])
        .background(color::GALLERY_PANEL)
        .border(
            Border::uniform(color::GALLERY_PANEL_RIM, size::BORDER)
                .side_color(0, PANEL_RIM_TOP)
                .side_color(2, PANEL_RIM_BOTTOM),
        )
        .radius(size::PANEL_RADIUS)
        .box_shadow(shadow::GALLERY_PANEL.to_vec())
        .overflow(Overflow::Clip)
        .position(Position::Absolute)
        .inset([
            Some(layout.panels_at[index].y),
            None,
            None,
            Some(layout.panels_at[index].x),
        ])
        .width(extent.x)
        .height(extent.y)
        .child(place(
            title,
            [
                Vec2::new(inset, 0.0),
                Vec2::new(
                    extent.x - 2.0 * size::BORDER - 2.0 * inset,
                    layout.title_height,
                ),
            ],
        ))
}

/// A line of words in `style` and `tint`, centred in `area`.
fn words(words: &str, style: TypeStyle, tint: [u8; 4], area: [Vec2; 2]) -> Element {
    // Text in the web's box widths can come out wider in Beryl 3D's faces (the web sets the
    // captions in italic Times New Roman, which is narrower than the upright face drawn), so
    // each line shrinks to stay on one line instead of wrapping out of its box.
    place(
        text(words)
            .style(style)
            .color(tint)
            .text_align(TextAlign::Center)
            .fit(true),
        area,
    )
}

/// A caption centred on `point.x`, in the web's 240-point `CaptionAt` box.
fn caption_at(layout: &GalleryLayout, name: &str, point: Vec2) -> Element {
    words(
        name,
        type_style::GALLERY_CAPTION,
        color::TYPE,
        [
            Vec2::new(point.x - layout.caption.x / 2.0, point.y),
            layout.caption,
        ],
    )
}

/// A group named `name` that fills its parent's padding box and draws nothing, so that the
/// parts in it stand where they would in the parent. It keeps samples of the same name, such as
/// a prompt's buttons, apart from those of the next sample.
fn apart(name: &str) -> Element {
    holder().name(name).position(Position::Absolute).inset([
        Some(0.0),
        Some(0.0),
        Some(0.0),
        Some(0.0),
    ])
}

/// An engraved rule across a panel: `.panel-rule`.
fn rule(area: [Vec2; 2]) -> Element {
    place(
        holder()
            .background_image(vec![gradient::GALLERY_RULE])
            .box_shadow(shadow::GALLERY_RULE.to_vec()),
        area,
    )
}

/// An upright divider between samples: `.specimen-divider`.
fn divider(area: [Vec2; 2]) -> Element {
    place(
        holder().background_image(vec![gradient::GALLERY_DIVIDER]),
        area,
    )
}

/// `01 Foundations`' five color swatches, each a chip over its name and hex value.
fn swatches(layout: &GalleryLayout, mut panel: Element) -> Element {
    for (index, (name, hex, fill, rim)) in SWATCHES.into_iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "five swatches")]
        let left = layout.chip[0].x + layout.chip_step * index as f32;
        let dark = index < 2;
        let chip = Element::new(Role::Region)
            .name(&format!("{name} swatch"))
            .background(fill)
            .border((rim, if dark { size::BORDER } else { 1.0 }))
            .radius(size::GALLERY_CHIP_RADIUS)
            .box_shadow(if dark {
                shadow::GALLERY_SWATCH.to_vec()
            } else {
                shadow::GALLERY_SWATCH_LIGHT.to_vec()
            });
        let [x, width] = layout.swatch_labels[index].to_array();
        panel = panel
            .child(place(
                chip,
                [Vec2::new(left, layout.chip[0].y), layout.chip[1]],
            ))
            .child(place(
                // The hex value is part of the swatch's caption, as `<small>` is on the web.
                text(name)
                    .name(&format!("{name}{hex}"))
                    .style(type_style::GALLERY_SWATCH)
                    .color(color::TYPE)
                    .text_align(TextAlign::Center)
                    // Its name keeps to its first line, over the hex value.
                    .first_line(layout.swatch_name.y - layout.swatch_line)
                    .child(words(
                        hex,
                        type_style::GALLERY_HEX,
                        color::TYPE,
                        [
                            Vec2::new(0.0, layout.swatch_hex.x),
                            Vec2::new(width, layout.swatch_hex.y),
                        ],
                    )),
                [
                    Vec2::new(x, layout.swatch_name.x),
                    Vec2::new(width, layout.swatch_name.y),
                ],
            ));
    }
    panel
}

fn foundations(layout: &GalleryLayout) -> Element {
    swatches(layout, panel(layout, 0))
        .child(words(
            "Scry",
            type_style::GALLERY_SERIF,
            color::TYPE,
            layout.serif_specimen,
        ))
        .child(words(
            "Serif · titles and actions",
            type_style::GALLERY_SPECIMEN_CAPTION,
            color::TYPE,
            layout.serif_caption,
        ))
        .child(divider(layout.specimen_divider))
        // Beryl 3D's sans serif is a little wider than the web's, so the specimen shrinks to
        // stay on one line.
        .child(
            words(
                "Drag cards to reorder",
                type_style::GALLERY_SANS,
                [255; 4],
                layout.sans_specimen,
            )
            .fit(true),
        )
        .child(words(
            "Sans serif · instructions",
            type_style::GALLERY_SPECIMEN_CAPTION,
            color::TYPE,
            layout.sans_caption,
        ))
        .child(place(
            Element::new(Role::Region)
                .name("matte")
                .background([0; 4])
                .background_image(vec![gradient::GALLERY_TABLETOP])
                .border(
                    Border::uniform([214, 172, 118, 255], size::BORDER)
                        .side_color(0, [223, 195, 157, 255])
                        .side_color(2, [205, 156, 96, 255]),
                )
                .radius(size::GALLERY_TABLETOP_RADIUS)
                .box_shadow(shadow::GALLERY_TABLETOP.to_vec()),
            layout.tabletop,
        ))
        .child(
            words(
                "Tabletop (matte)",
                type_style::GALLERY_MATERIAL_CAPTION,
                color::TYPE,
                layout.tabletop_caption,
            )
            .fit(true),
        )
        .child(place(
            holder()
                .name("gold")
                .background_image(vec![gradient::GALLERY_METAL_GOLD])
                .box_shadow(shadow::GALLERY_METAL_GOLD.to_vec()),
            [layout.metal_rules[0], layout.metal_rule],
        ))
        .child(place(
            holder()
                .name("silver")
                .background_image(vec![gradient::GALLERY_METAL_SILVER])
                .box_shadow(shadow::GALLERY_METAL_SILVER.to_vec()),
            [layout.metal_rules[1], layout.metal_rule],
        ))
        .child(words(
            "Thin metallic borders",
            type_style::GALLERY_METAL_CAPTION,
            color::TYPE,
            layout.metal_caption,
        ))
}

fn actions(layout: &GalleryLayout) -> Element {
    let samples = [
        place(
            primary("Done").style(type_style::GALLERY_PRIMARY),
            layout.done,
        ),
        place(
            button("View Battlefield").style(type_style::GALLERY_SECONDARY),
            layout.view,
        ),
        place(
            primary("Pass")
                .enabled(false)
                .style(type_style::GALLERY_DISABLED),
            layout.pass,
        ),
    ];
    let captions = ["Primary", "Secondary", "Disabled"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            // Each caption's box is exactly the web's text width, which Beryl 3D's italic face
            // can overrun by a fraction of a point, so the caption shrinks to stay inside it.
            place(
                text(name)
                    .style(type_style::GALLERY_ROW_CAPTION)
                    .color(color::TYPE)
                    .fit(true),
                [layout.row_captions[index], layout.row_caption_sizes[index]],
            )
        });
    let pill = place(
        Element::new(Role::Region)
            .name("Opponent has priority")
            .background([37, 32, 29, 255])
            .border(
                Border::uniform([165, 158, 152, 255], size::BORDER)
                    .side_color(0, [179, 172, 166, 255]),
            )
            .radius(layout.pill[1].y / 2.0)
            .box_shadow(shadow::GALLERY_PILL.to_vec())
            .child(words(
                "Opponent has priority",
                type_style::GALLERY_PILL,
                [247, 244, 239, 255],
                layout.pill_text,
            )),
        layout.pill,
    );
    panel(layout, 1)
        .children(samples)
        .children(captions)
        .children(
            layout
                .actions_rules
                .map(|point| rule([point, layout.actions_rule])),
        )
        .child(pill)
        .child(place(phase_tracker(layout), layout.tracker))
}

/// The phase tracker of `02 Actions & status`, lighting combat: `PhaseTracker` with
/// `current="combat"`, its five icons at the web's places in its pill.
fn phase_tracker(layout: &GalleryLayout) -> Element {
    let [point, extent] = layout.phase;
    let mut tracker = Element::new(Role::List)
        .name("Phase tracker")
        .background([0; 4])
        .background_image(vec![gradient::PHASE_TRACKER])
        .border(
            Border::uniform([90, 84, 78, 255], 1.0)
                .side_color(0, [138, 131, 124, 255])
                .side_color(2, [138, 131, 124, 255]),
        )
        .radius(layout.tracker[1].y / 2.0)
        .box_shadow(shadow::GALLERY_TRACKER.to_vec());
    for (index, (icon, label)) in icons::PHASES
        .into_iter()
        .zip([
            "Upkeep",
            "First main phase",
            "Combat",
            "Second main phase",
            "End",
        ])
        .enumerate()
    {
        let current = label == "Combat";
        let mut sample = Element::new(Role::Icon)
            .name(label)
            .color(if current {
                color::GOLD
            } else {
                color::TEXT_DIM
            })
            .drop_shadow(if current {
                shadow::CURRENT_GLOW.to_vec()
            } else {
                shadow::PHASE_ICON.to_vec()
            });
        sample.shows = Some(Shown::Icon(icon, icons::PHASE_SIDE));
        #[expect(clippy::cast_precision_loss, reason = "five phases")]
        let left = point.x + layout.phase_step * index as f32;
        tracker = tracker.child(place(sample, [Vec2::new(left, point.y), extent]));
    }
    tracker
}

fn player_zones(layout: &GalleryLayout, faces: &FaceList) -> Element {
    let [_, _, art_width, art_height] = game_hud::art::orb::ART;
    let units = game_hud::art::orb::SCALES[0];
    let art = Vec2::new(art_width, art_height) * units;
    let orbs = [
        (Side::You, true, "22", "Active"),
        (Side::Opponent, false, "17", "Inactive"),
    ];
    let mut panel = panel(layout, 2);
    for (index, (side, active, life, name)) in orbs.into_iter().enumerate() {
        // Beryl 3D's orb art comes at the table's scale, larger than the web gallery's
        // (`--orb: 0.74px`), so it is centred on the web's orb.
        let middle = layout.orbs[index] + layout.orb / 2.0;
        let mut orb = Element::new(Role::Region).name(&format!("{name} orb"));
        orb.shows = Some(Shown::Layers(artwork(side, active, 0)));
        panel = panel
            .child(place(orb, [middle - art / 2.0, art]))
            .child(words(
                life,
                type_style::GALLERY_LIFE,
                color::LIFE,
                [layout.lives[index], layout.life_sizes[index]],
            ))
            .child(caption_at(layout, name, layout.orb_captions[index]));
    }
    let badge = place(
        Element::new(Role::Region)
            .name("Opponent")
            .background([0; 4])
            .background_image(vec![gradient::GALLERY_BADGE])
            .child(place(
                text("Opponent")
                    .style(type_style::GALLERY_BADGE)
                    .color([255; 4]),
                layout.badge_text,
            )),
        layout.badge,
    );
    let count = place(
        text("×3")
            .style(type_style::GALLERY_LAND_COUNT)
            .color(color::GALLERY_LAND_COUNT)
            .text_align(TextAlign::Center)
            .background([0; 4])
            .background_image(vec![gradient::GALLERY_LAND_COUNT])
            .border(([184, 146, 78, 255], size::BORDER))
            .radius(size::GALLERY_LAND_COUNT_RADIUS)
            .box_shadow(shadow::GALLERY_LAND_COUNT.to_vec()),
        layout.land_count,
    );
    panel
        .child(divider(layout.zones_divider))
        .child(badge)
        .child(rule(layout.zones_rule))
        .child(pile(
            "Library, 40 cards",
            CARD_BACK,
            layout.library,
            (
                layout.library_card,
                layout.library_depth,
                layout.library_layers,
            ),
            ([26, 23, 21, 255], [12, 10, 9, 255], [58, 51, 46, 255]),
        ))
        .child(pile(
            "Graveyard, 4 cards",
            face(faces, "Dusk Cut"),
            layout.graveyard,
            (
                layout.graveyard_card,
                layout.graveyard_depth,
                layout.graveyard_layers,
            ),
            ([20, 18, 16, 255], [74, 68, 63, 255], [97, 90, 83, 255]),
        ))
        .children(layout.lands.into_iter().enumerate().map(|(index, point)| {
            apart(&format!("land {}", index + 1)).child({
                place(
                    flat_face(face(faces, "Island"), "Island"),
                    [point, layout.land],
                )
            })
        }))
        .child(count)
        .children(
            ["Library", "Graveyard", "Lands (stacked)"]
                .into_iter()
                .zip(layout.pile_captions)
                .map(|(name, point)| caption_at(layout, name, point)),
        )
}

/// A pile drawn with depth, as the web's `Library` and `Graveyard` with a `depth` draw one:
/// `layers` cards' edges stepping up and right from the pile's bottom left corner, under its top
/// card, `top`. `fill`, `rim` and `foot` are each edge's face, border and lower border.
fn pile(
    name: &str,
    top: FaceId,
    area: [Vec2; 2],
    (card, depth, layers): (Vec2, Vec2, usize),
    (fill, rim, foot): ([u8; 4], [u8; 4], [u8; 4]),
) -> Element {
    #[expect(clippy::cast_precision_loss, reason = "a pile's few layers")]
    let step = |layer: usize| {
        let share = layer as f32 / layers as f32;
        Vec2::new(depth.x * share, depth.y * (1.0 - share))
    };
    let edges = (0..layers).map(|layer| {
        let edge = Element::new(Role::Region)
            .background(fill)
            .border(Border::uniform(rim, 1.0).side_color(2, foot))
            .radius(size::GALLERY_PILE_RADIUS);
        // Only the lowest card casts the pile's shadow.
        let edge = if layer == 0 {
            edge.box_shadow(shadow::GALLERY_PILE_FOOT.to_vec())
        } else {
            edge
        };
        place(edge, [step(layer), card])
    });
    place(
        holder()
            .name(name)
            .children(edges)
            .child(place(flat_face(top, "top card"), [step(layers), card])),
        area,
    )
}

/// A card face, drawn flat, named `name`.
fn flat_face(id: FaceId, name: &str) -> Element {
    let mut card = Element::new(Role::Region).name(name);
    card.shows = Some(Shown::Face(id, FaceSize::Board));
    card
}
