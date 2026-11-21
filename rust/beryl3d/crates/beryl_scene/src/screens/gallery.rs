//! The web component gallery's samples. On the desktop they stand on the web's page of numbered
//! panels (`desktop`); on a phone they stand in one scrolling column.

mod desktop;
mod samples;

pub(crate) use samples::faces;

use crate::screens::ScreenPress;
use glam::Vec2;

use super::{Screen, ScreenIn};
use crate::element::{
    Action, Align, Anchor, Element, Justify, Overflow, Shown, TextAlign, column, row, text,
};
use crate::faces::{CARD_BACK, FaceList};
use crate::input::{FaceId, FaceSize};
use crate::inspect::viewer::scry_sample;
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::widgets::{MenuGroup, MenuId, MenuRow, button, context_menu, icons, menu, primary};
use kit_base::data::space::flat::gallery as metrics;
use kit_base::tokens::{color, shadow, size, space, type_style};
use kit_frame::frame::Role;

/// A context menu sample opened in the gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalleryMenu {
    Permanent,
    Library,
    Corner,
}

/// What the gallery shows from the command list's three example queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GalleryQuery {
    Empty,
    Top,
    Untap,
}

/// Which interactive samples are open on the gallery.
#[derive(Clone, Copy, Debug, Default)]
pub struct GalleryState {
    pub(crate) menu: Option<(GalleryMenu, Vec2)>,
    pub(crate) query: Option<GalleryQuery>,
}

const MENU_ID: MenuId = MenuId(1);

/// The component library at `#/gallery` in the web interface.
pub fn gallery(input: &ScreenIn) -> Element {
    let GalleryState {
        menu: opened,
        query,
    } = input.gallery;
    let samples;
    let faces = if let Some(faces) = input.faces {
        faces
    } else {
        samples = faces();
        &samples
    };
    let inset = input.window.safe_area;
    let width = (input.window.size.width - inset.left - inset.right).max(0.0);
    let height = (input.window.size.height - inset.top - inset.bottom).max(0.0);
    if !input.window.phone() {
        return on_desktop(input, faces, width, height);
    }
    // On a phone the samples stand in one column the phone's gutter in from the safe area's
    // sides, as the other portrait screens do; the sample rows wrap to that width.
    let edge = if input.window.phone() {
        kit_base::data::space::flat::phone::GUTTER
    } else {
        space::L
    };
    let content = column()
        .width((width - 2.0 * edge).clamp(0.0, metrics::WIDTH))
        .shrink(0.0)
        .align_items(Align::Center)
        .gap(space::L)
        .child(
            text("Project Beryl")
                .style(type_style::TITLE)
                .color(color::GOLD),
        )
        .child(text("Game UI · Component Library").style(type_style::OVERLAY_INSTRUCTION))
        .child(foundations())
        .child(actions())
        .child(player_zones(faces))
        .child(card_states(faces))
        .child(combat(faces))
        .child(
            section("Scry overlay").child(scry_sample(faces)).child(
                specimen_row().children(
                    [
                        "Drag & reorder",
                        "Source card",
                        "Confirm",
                        "Dimmed table backdrop",
                    ]
                    .map(caption),
                ),
            ),
        )
        .child(prompts())
        .child(context_menus(opened.is_none()))
        .child(commands(query, opened.is_none()));
    let mut screen = column()
        .name("gallery")
        .width(width)
        .height(height)
        .background(color::TABLE)
        .overflow(Overflow::Scroll)
        .align_items(Align::Center)
        .padding(edge)
        .child(content)
        .shortcut(
            "Escape",
            if opened.is_some() {
                ScreenPress::DismissMenu
            } else if query.is_some() {
                ScreenPress::GalleryQuery(None)
            } else {
                ScreenPress::Open(Screen::Start)
            },
        );
    // The gallery fills the safe area. The game screen's root reserves room for its dock.
    screen.anchor = Some(Anchor::Point(Vec2::new(inset.left, inset.top), Vec2::ZERO));
    if let Some((kind, at)) = opened {
        let at = if kind == GalleryMenu::Corner {
            Vec2::new(input.window.size.width, input.window.size.height)
        } else {
            at
        };
        screen = screen.child(context_menu(at, example_menu(kind)));
    }
    screen
}

/// The gallery on the desktop: the web's page, centred in the window, which scrolls to show
/// its lower panels. A menu opens where its button was pressed, and the command list's preview
/// stands in the middle of the window, as the web's `CommandList` panel does.
fn on_desktop(input: &ScreenIn, faces: &FaceList, width: f32, height: f32) -> Element {
    let GalleryState {
        menu: opened,
        query,
    } = input.gallery;
    let inset = input.window.safe_area;
    let mut screen = column()
        .name("gallery")
        .width(width)
        .height(height)
        .background(color::GALLERY_PAGE)
        .overflow(Overflow::Scroll)
        .align_items(Align::Center)
        .child(desktop::page(faces, opened.is_none() && query.is_none()))
        .shortcut("Escape", escape(input.gallery));
    screen.anchor = Some(Anchor::Point(Vec2::new(inset.left, inset.top), Vec2::ZERO));
    if let Some(query) = query {
        let mut preview = command_preview(query)
            .padding(space::L)
            .background(color::SURFACE)
            .border((color::METAL, size::BORDER))
            .radius(size::PANEL_RADIUS);
        preview.anchor = Some(Anchor::Point(
            Vec2::new(input.window.size.width, input.window.size.height) / 2.0,
            Vec2::splat(0.5),
        ));
        screen = screen.child(preview);
    }
    if let Some((kind, at)) = opened {
        screen = screen.child(context_menu(
            menu_point(input, kind, at),
            example_menu(kind),
        ));
    }
    screen
}

/// What Escape does on the gallery: closes an open menu, then the command list's preview, then
/// goes back to the start screen.
fn escape(state: GalleryState) -> ScreenPress {
    if state.menu.is_some() {
        ScreenPress::DismissMenu
    } else if state.query.is_some() {
        ScreenPress::GalleryQuery(None)
    } else {
        ScreenPress::Open(Screen::Start)
    }
}

/// Where menu `kind` opens: at the point its button gave, or at the window's bottom right corner
/// for the corner sample.
fn menu_point(input: &ScreenIn, kind: GalleryMenu, at: Vec2) -> Vec2 {
    if kind == GalleryMenu::Corner {
        Vec2::new(input.window.size.width, input.window.size.height)
    } else {
        at
    }
}

fn section(name: &str) -> Element {
    column()
        .name(name)
        .align_self(Align::Stretch)
        .shrink(0.0)
        .gap(space::M)
        .padding(space::L)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .child(text(name).style(type_style::MENU_TITLE))
}

fn specimen_row() -> Element {
    row()
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .gap(space::L)
        .wrap(true)
}

fn caption(name: &str) -> Element {
    text(name).style(type_style::BODY).color(color::TEXT_DIM)
}

fn specimen(item: Element, name: &str) -> Element {
    column()
        .gap(space::S)
        .align_items(Align::Center)
        .child(item)
        .child(caption(name))
}

fn foundations() -> Element {
    let swatches = [
        ("Table", color::TABLE),
        ("Surface", color::SURFACE),
        ("Text", color::TEXT),
        ("Gold", color::GOLD),
        ("Cyan", color::CYAN),
    ];
    section("Foundations")
        .child(
            specimen_row().children(swatches.map(|(name, [r, g, b, a])| {
                let chip = row()
                    .name(name)
                    .width(metrics::CHIP)
                    .height(space::L)
                    .background([r, g, b, a]);
                specimen(chip, name).child(caption(&format!("#{r:02X}{g:02X}{b:02X}")))
            })),
        )
        .child(specimen_row().children([
            specimen(
                text("Scry").style(type_style::TITLE),
                "Serif · titles and actions",
            ),
            specimen(
                text("Drag cards to reorder").style(type_style::BODY),
                "Sans serif · instructions",
            ),
        ]))
        .child(
            specimen_row().children([
                specimen(
                    row()
                        .name("matte")
                        .width(metrics::CHIP)
                        .height(space::M)
                        .background(color::TABLE),
                    "Tabletop (matte)",
                ),
                specimen(
                    row()
                        .name("metal")
                        .width(metrics::CHIP)
                        .height(space::M)
                        .border((color::METAL, size::BORDER)),
                    "Thin metallic borders",
                ),
            ]),
        )
}

fn actions() -> Element {
    section("Actions & status")
        .child(specimen_row().children([
            specimen(primary("Done"), "Primary"),
            specimen(button("View Battlefield"), "Secondary"),
            specimen(primary("Pass").enabled(false), "Disabled"),
        ]))
        .child(specimen_row().child(pill("Opponent has priority", color::TEXT_DIM)))
        .child(specimen_row().child(phase_tracker()))
}

/// The phase tracker's thin grey rim, the top and bottom color of `.phase-tracker`'s border in
/// `ui/src/design/components/phase-tracker.css` (its sides are darker on the web).
const PHASE_TRACKER_RIM: [u8; 4] = [0x8a, 0x83, 0x7c, 0xff];

fn phase_tracker() -> Element {
    let side = f32::from(icons::PHASE_SIDE);
    row()
        .name("Phase tracker")
        .padding(space::S)
        .gap(space::S)
        .background([0; 4])
        .background_image(vec![kit_base::tokens::gradient::PHASE_TRACKER])
        .border((PHASE_TRACKER_RIM, 1.0))
        .radius(size::PILL_HEIGHT / 2.0)
        .children(
            icons::PHASES
                .into_iter()
                .zip([
                    "Upkeep",
                    "First main phase",
                    "Combat",
                    "Second main phase",
                    "End",
                ])
                .map(|(icon, label)| {
                    let mut sample = row()
                        .name(label)
                        .width(side)
                        .height(side)
                        .color(if label == "Combat" {
                            color::GOLD
                        } else {
                            color::TEXT_DIM
                        })
                        // The web gallery's tracker lights combat
                        // (`.phase-current .phase-icon`).
                        .drop_shadow(if label == "Combat" {
                            shadow::CURRENT_GLOW.to_vec()
                        } else {
                            shadow::PHASE_ICON.to_vec()
                        });
                    sample.shows = Some(Shown::Icon(icon, icons::PHASE_SIDE));
                    sample
                }),
        )
}

fn pill(label: &str, tint: [u8; 4]) -> Element {
    text(label)
        .style(type_style::PILL)
        .padding(space::S)
        .min_width(size::PILL_HEIGHT)
        .text_align(TextAlign::Center)
        .background(color::SURFACE)
        .border((tint, size::BORDER))
        .radius(size::PILL_HEIGHT / 2.0)
        .color(tint)
}

fn player_zones(faces: &FaceList) -> Element {
    section("Player & zones")
        .child(specimen_row().children([
            specimen(pill("22", color::GOLD), "Active"),
            specimen(pill("17", color::TEXT_DIM), "Inactive"),
            pill("Opponent", color::TEXT_DIM),
        ]))
        .child(specimen_row().children([
            specimen(pile(CARD_BACK, "Card back", 40), "Library"),
            specimen(pile(face(faces, "Dusk Cut"), "Dusk Cut", 4), "Graveyard"),
            specimen(pile(face(faces, "Island"), "Island", 3), "Lands (stacked)"),
        ]))
}

fn pile(id: FaceId, name: &str, count: u8) -> Element {
    column()
        .gap(space::S)
        .align_items(Align::Center)
        .child(flat_card(id, name, metrics::SMALL_CARD))
        .child(pill(&count.to_string(), color::TEXT_DIM))
}

fn card_states(faces: &FaceList) -> Element {
    section("Cards & states")
        .child(specimen_row().children([
            card(faces, "Cinder Colossus", metrics::SMALL_CARD),
            specimen(tapped_card(faces, "Mountain"), "Tapped"),
            specimen(card(faces, "Plains", metrics::SMALL_CARD), "Untapped"),
            specimen(
                rim(
                    card(faces, "Gloom Wren", metrics::SMALL_CARD),
                    color::GOLD,
                    size::PANEL_RADIUS,
                ),
                "Source (gold)",
            ),
            specimen(
                rim(
                    card(faces, "Gloom Wren", metrics::SMALL_CARD),
                    color::CYAN,
                    size::PANEL_RADIUS,
                ),
                "Target (cyan)",
            ),
        ]))
        .child(
            specimen_row().children(
                ["Exile Lance", "Mountain", "Plateau"]
                    .map(|name| card(faces, name, metrics::HAND_CARD)),
            ),
        )
}

fn combat(faces: &FaceList) -> Element {
    const TRIGGER: &str = "When Cinder Colossus enters the battlefield, it deals 3 damage divided as you choose among one, two, or three targets.";
    section("Combat & stack")
        .child(specimen_row().children([
            specimen(
                rim(card(faces, "Beast", metrics::SMALL_CARD), color::EMBER, 0.0),
                "Attacker · angular",
            ),
            specimen(
                rim(
                    card(faces, "Angel", metrics::SMALL_CARD),
                    color::CYAN,
                    size::PANEL_RADIUS,
                ),
                "Blocker · rounded",
            ),
        ]))
        .child(
            specimen_row().children([
                card(faces, "Cinder Colossus", metrics::SMALL_CARD),
                specimen(
                    flat_card(
                        faces
                            .plate("Cinder Colossus", true, 0)
                            .expect("the Colossus has a trigger"),
                        "Cinder Colossus ability",
                        metrics::HAND_CARD,
                    ),
                    TRIGGER,
                ),
                card(faces, "Beast", metrics::SMALL_CARD),
            ]),
        )
}

fn rim(card: Element, tint: [u8; 4], radius: f32) -> Element {
    column()
        .padding(size::BORDER)
        .border((tint, size::BORDER))
        .radius(radius)
        .child(card)
}

fn face(faces: &FaceList, name: &str) -> FaceId {
    faces
        .find(name)
        .expect("the gallery lists every sample face")
}

fn card(faces: &FaceList, name: &str, width: f32) -> Element {
    flat_card(face(faces, name), name, width)
}

fn tapped_card(faces: &FaceList, name: &str) -> Element {
    let width = metrics::SMALL_CARD;
    let face = face(faces, name);
    let mut card = flat_card(face, name, width)
        .width(width * CARD_HEIGHT / CARD_WIDTH)
        .height(width);
    card.shows = Some(Shown::TappedFace(face, FaceSize::Board));
    card
}

fn flat_card(id: FaceId, name: &str, width: f32) -> Element {
    let mut card = Element::new(Role::Region)
        .name(name)
        .width(width)
        .height(width * CARD_HEIGHT / CARD_WIDTH)
        .shrink(0.0)
        .background(color::SURFACE);
    card.shows = Some(Shown::Face(id, FaceSize::Board));
    card
}

fn prompts() -> Element {
    let prompt = |name, entries: Vec<Element>, confirm| {
        column()
            .name(name)
            .width(metrics::PROMPT_WIDTH)
            .gap(space::S)
            .padding(space::M)
            .background(color::SURFACE)
            .radius(size::PANEL_RADIUS)
            .children(entries)
            .child(row().gap(space::S).children(
                [button("Cancel"), primary(confirm)].map(|button| button.min_width(0.0).grow(1.0)),
            ))
    };
    let value = |label, words| {
        column().gap(space::S).child(caption(label)).child(
            text(words)
                .style(type_style::FIELD)
                .padding(space::S)
                .border((color::METAL, size::FIELD_BORDER))
                .radius(size::FIELD_RADIUS),
        )
    };
    section("Manual table prompts").child(specimen_row().children([
        prompt("Draw cards", vec![value("Cards to draw", "1")], "Draw 1"),
        prompt("Set note", vec![value("Note", "Doesn't untap")], "Set note"),
        prompt(
            "Set power and toughness…",
            vec![
            row().gap(space::S).children([value("Power", "3"), value("Toughness", "3")]),
        ],
            "Set 3/3",
        ),
    ]))
}

fn context_menus(available: bool) -> Element {
    section("Context menus").child(
        specimen_row().children(
            [
                ("Permanent", GalleryMenu::Permanent),
                ("Library", GalleryMenu::Library),
                ("Bottom-right corner", GalleryMenu::Corner),
            ]
            .map(|(label, kind)| example_button(label, ScreenPress::GalleryMenu(kind), available)),
        ),
    )
}

fn example_button(label: &str, press: ScreenPress, available: bool) -> Element {
    let mut button = button(label);
    button.press = available.then(|| Action::new(press));
    button
}

fn example_menu(kind: GalleryMenu) -> Element {
    let group = |rows: &[(&str, &str)]| MenuGroup {
        rows: rows
            .iter()
            .map(|&(label, key)| MenuRow {
                key: (!key.is_empty()).then(|| key.to_owned()),
                ..MenuRow::new(label)
            })
            .collect(),
    };
    if kind == GalleryMenu::Permanent {
        return menu(
            MENU_ID,
            "Beast",
            "Token creature · yours",
            &[
                group(&[("Tap", "")]),
                group(&[
                    ("Graveyard", "Cmd+Backspace"),
                    ("Exile", ""),
                    ("Hand", ""),
                    ("Top of library", ""),
                    ("Bottom of library", "Cmd+B"),
                ]),
                group(&[
                    ("Add counter…", ""),
                    ("Set power and toughness…", "Cmd+P"),
                    ("Every action…", "Cmd+K"),
                ]),
            ],
        );
    }
    let mut rows = group(&[
        ("Keep top card revealed", "Cmd+N"),
        ("Bottom of library", ""),
        ("Every action…", "Cmd+K"),
    ]);
    rows.rows[0].checked = true;
    rows.rows[1].submenu = group(&[
        ("Draw bottom card", ""),
        ("Look at bottom cards…", "Shift+Cmd+W"),
        ("Move bottom card to graveyard", ""),
    ])
    .rows;
    menu(
        MENU_ID,
        "Library",
        "53 cards · yours",
        &[
            group(&[
                ("Draw", ""),
                ("Draw cards…", "Cmd+E"),
                ("Look at top cards…", "Cmd+W"),
                ("Search", "F3"),
                ("Shuffle", "Cmd+S"),
            ]),
            group(&[("Scry…", ""), ("Surveil…", "")]),
            rows,
        ],
    )
}

fn commands(query: Option<GalleryQuery>, available: bool) -> Element {
    let buttons = specimen_row().children(
        [
            ("Empty query", GalleryQuery::Empty),
            ("Two groups", GalleryQuery::Top),
            ("No matches", GalleryQuery::Untap),
        ]
        .map(|(label, query)| {
            example_button(label, ScreenPress::GalleryQuery(Some(query)), available)
        }),
    );
    section("Command list")
        .child(buttons)
        .children(query.map(command_preview))
}

fn command_preview(query: GalleryQuery) -> Element {
    type Group = (&'static str, &'static [&'static str]);
    let (value, groups): (&str, &[Group]) = match query {
        GalleryQuery::Empty => (
            "",
            &[
                ("Library", &["Draw cards…", "Look at top cards…", "Shuffle"]),
                ("Selected card", &["Move to top of library", "Tap"]),
            ],
        ),
        GalleryQuery::Top => (
            "top",
            &[
                ("Library", &["Look at top cards…"]),
                ("Selected card", &["Move to top of library"]),
            ],
        ),
        GalleryQuery::Untap => ("untap", &[]),
    };
    let mut preview = column()
        .name("Commands")
        .gap(space::S)
        .child(caption("Your library"))
        .child(pill(
            if value.is_empty() {
                "Search commands"
            } else {
                value
            },
            color::TEXT_DIM,
        ));
    for &(name, rows) in groups {
        preview = preview
            .child(caption(name))
            .children(rows.iter().map(|label| text(label)));
    }
    if groups.is_empty() {
        preview = preview.child(text("No matching commands"));
    }
    preview
}
