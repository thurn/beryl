//! The life orbs as the web interface draws them (`LifeOrb` in `ui/src/components/Player.tsx`),
//! in a desktop window at two pixels to a point, as on the Retina display the owner reported
//! them from: each orb draws the layers of the web's art, its ring toward its player's edge of
//! the table and the active player's flare, with no panel behind it or its life total, and the
//! step stops stand clear of it as the web's grid places them.

use app_client::Input;
use beryl_protocol::prompt::{Marked, Purpose, ShapeBody};
use beryl_protocol::protocol::{CardType, Kind, ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use game_hud::art::orb::{self, LAYERS};
use game_play::art::orb::face_id;
use game_table::profiles::Profile;
use game_table::table::Window;
use glam::{Vec2, Vec4};
use kit_base::data::profiles::Profiles;
use kit_base::tokens::{color, size};
use kit_frame::frame::{Band, ElementId, Frame, Material, Role, ShapeAnchor, StateBits};
use kit_probe::agent::checks::ring_rect;
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{AssetReady, IconCell, Insets, Size};

use super::flat_layer::{client, deciding, edges, find, prompt};

/// A cell for every layer of the art at every scale, each at its own column of page 1, so that
/// a draw's `uv` names the layer it draws.
fn orb_cells(profiles: &Profiles) -> Vec<IconCell> {
    orb::atlas_cells(profiles)
        .enumerate()
        .map(|(index, layer)| layer.cell([u16::try_from(index).unwrap(), 0, 1, 1]))
        .collect()
}

/// The middle of a flat-layer draw on the screen, in logical points.
fn screen_centre(frame: &Frame, index: usize, window: Vec2) -> Vec2 {
    let model = &frame.draws[index].model;
    let clip = frame.view_projection * Vec4::new(model[3], model[7], model[11], 1.0);
    let ndc = Vec2::new(clip.x, clip.y) / clip.w;
    Vec2::new(
        f32::midpoint(ndc.x, 1.0) * window.x,
        f32::midpoint(1.0, -ndc.y) * window.y,
    )
}

/// The layers of `side`'s desktop orb that `frame` draws, by index in `LAYERS`, in the order
/// drawn, each with its middle on the screen.
fn drawn(frame: &Frame, side: Side, window: Vec2) -> Vec<(usize, Vec2)> {
    // The desktop's cells come first, the viewer's orb's before the opponent's.
    let first = usize::from(side == Side::Opponent) * LAYERS.len();
    (frame.layer as usize..frame.draws.len())
        .filter(|&index| frame.draws[index].material == Material::Icon as u8)
        .filter(|&index| frame.draws[index].layer == 1)
        .filter_map(|index| {
            let column = usize::from(frame.draws[index].uv[0]);
            (first..first + LAYERS.len())
                .contains(&column)
                .then(|| (column - first, screen_centre(frame, index, window)))
        })
        .collect()
}

#[test]
fn each_orb_draws_the_webs_art_with_its_ring_toward_its_edge() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.handle(Input::Device(DeviceInput::Assets(AssetReady::Icons {
        page: 1,
        cells: orb_cells(&client.data().profiles),
    })));
    let frame = client.frame(0);
    let size = Vec2::new(1440.0, 900.0);
    let core = LAYERS
        .iter()
        .position(|layer| layer.shape == orb::Shape::Core && layer.paint.stroke.is_none())
        .expect("a core layer");
    let ring = LAYERS
        .iter()
        .rposition(|layer| matches!(layer.shape, orb::Shape::Rim { offset, .. } if offset > 0.0))
        .expect("a ring layer");
    let mut cores = Vec::new();
    for (side, name) in [(Side::You, "your orb"), (Side::Opponent, "opponent's orb")] {
        let element = find(frame, Role::Region, name);
        let active = frame.resolved[element].state.has(StateBits::SELECTED);
        let layers = drawn(frame, side, size);
        let want: Vec<usize> = (0..LAYERS.len())
            .filter(|&at| active || !LAYERS[at].active)
            .collect();
        assert_eq!(
            layers.iter().map(|&(at, _)| at).collect::<Vec<_>>(),
            want,
            "{name} draws every layer of its art in order"
        );
        let place = |at: usize| layers.iter().find(|&&(drawn, _)| drawn == at).unwrap().1;
        cores.push(place(core));
        // The ring lies toward the player's edge: below the viewer's core, above the
        // opponent's.
        let below = place(ring).y > place(core).y;
        assert_eq!(below, side == Side::You, "{name}'s ring is toward its edge");
    }
    assert!(cores[0].y > cores[1].y);

    // No panel is drawn behind either life total: the web draws none on its orb.
    let totals = (0..frame.resolved.len()).filter(|&at| frame.resolved[at].role == Role::Player);
    for total in totals {
        let name = frame.name(frame.resolved[total].name);
        let (left, top, right, bottom) = edges(frame, total);
        let middle = Vec2::new(f32::midpoint(left, right), f32::midpoint(top, bottom));
        let behind = (frame.layer as usize..frame.draws.len())
            .filter(|&index| frame.draws[index].material == Material::Panel as u8)
            .any(|index| screen_centre(frame, index, size).distance(middle) < 20.0);
        assert!(!behind, "a panel is drawn behind {name}");
    }

    // Each side's step stops end 70 points from the middle of the viewer's core, as the web's
    // grid, 308 wide, places them.
    let core_x = cores[0].x;
    let (_, _, inner_left, _) = edges(frame, find(frame, Role::Toggle, "Your draw"));
    let (inner_right, ..) = edges(frame, find(frame, Role::Toggle, "Your blockers"));
    assert!((core_x - inner_left - 70.0).abs() < 1.0, "{inner_left}");
    assert!((inner_right - core_x - 70.0).abs() < 1.0, "{inner_right}");
}

/// A target prompt with one legal creature and the named players eligible.
fn target_prompt(sides: &[Side]) -> (Input, ObjectId) {
    let Input::Update(mut update) = deciding(prompt("Choose a target.")) else {
        unreachable!()
    };
    let card = update
        .view
        .objects
        .iter()
        .find(|object| {
            object.zone == ZoneName::Battlefield && object.types.contains(&CardType::Creature)
        })
        .unwrap()
        .slot;
    let Status::Deciding { decision } = &mut update.view.status else {
        unreachable!()
    };
    decision.kind = Kind::Target;
    decision.prompt.purpose = Purpose::Target;
    decision.prompt.body = ShapeBody::Pick {
        tray: Vec::new(),
        least: 1,
        most: 1,
        picked: Vec::new(),
        warned: Vec::new(),
    };
    decision.prompt.marks = sides
        .iter()
        .map(|&player| Subject::Player { player })
        .chain([Subject::Object { slot: card }])
        .map(|on| Marked {
            on,
            mark: Mark::Eligible,
        })
        .collect();
    (Input::Update(update), card)
}

#[test]
fn eligible_players_have_face_marks_and_eligible_cards_keep_their_outlines() {
    let window = Window {
        size: Size {
            width: 1728.0,
            height: 1117.0,
        },
        scale: 2.0,
        ..Window::default()
    };
    for sides in [&[Side::You, Side::Opponent][..], &[Side::Opponent][..]] {
        let mut client = client();
        client.handle(Input::Device(DeviceInput::Surface {
            size: window.size,
            scale: window.scale,
            safe_area: Insets::default(),
        }));
        let scale = Profile::for_window(&client.data().profiles, window)
            .data()
            .play
            .life
            .orb_scale;
        let (update, card) = target_prompt(sides);
        client.handle(Input::Device(DeviceInput::Assets(AssetReady::Icons {
            page: 1,
            cells: orb_cells(&client.data().profiles),
        })));
        client.handle(update);
        client.frame(0);
        client.frame(1_000_000_000);
        let frame = client.frame(2_000_000_000);
        let player_marks = frame.draws.iter().enumerate().filter(|(index, draw)| {
            frame.band(*index) == Some(Band::Layer)
                && draw.material == Material::Icon as u8
                && draw.tint[..3] == color::MARK_ELIGIBLE[..3]
        });
        assert_eq!(player_marks.count(), 2 * sides.len());

        for (side, name) in [(Side::You, "your orb"), (Side::Opponent, "opponent's orb")] {
            let mark = frame.rings.iter().find(|ring| {
                frame.decoration_of(ring.element) == Some(ShapeAnchor::Outline(face_id(side)))
            });
            assert_eq!(mark.is_some(), sides.contains(&side), "{name}");
            if let Some(mark) = mark {
                let ring = ring_rect(mark, frame.view_projection, window.size);
                let (left, top, right, bottom) = edges(frame, find(frame, Role::Region, name));
                let orb_middle = Vec2::new(f32::midpoint(left, right), f32::midpoint(top, bottom));
                let face_middle = orb_middle + Vec2::Y * orb::face_offset(side) * scale;
                let ring_middle = Vec2::new(ring.x + ring.width / 2.0, ring.y + ring.height / 2.0);
                assert!(ring_middle.distance(face_middle) < 0.1, "{name}: {ring:?}");
                assert!(
                    (ring.width - (size::ORB_WIDTH + size::ORB_BEZEL + 4.0) * scale).abs() < 0.1
                );
                assert!(
                    (ring.height - (size::ORB_HEIGHT + size::ORB_BEZEL + 4.0) * scale).abs() < 0.1
                );
                assert!(!frame.resolved[mark.element as usize].interactive);
            }
        }
        let card_mark = frame.rings.iter().find(|ring| {
            frame.decoration_of(ring.element)
                == Some(ShapeAnchor::Outline(ElementId(u32::from(card.0))))
        });
        assert!(card_mark.is_some_and(|ring| !ring.shadows.is_empty()));
    }
}
