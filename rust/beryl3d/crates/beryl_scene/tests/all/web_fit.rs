//! The desktop's battlefield rows, fitted to where the web interface draws them
//! (`table::web_fit`): each card's box on the window against the web's box of the same card.

use beryl_protocol::protocol::{CardType, Combat, ObjectId, Side, ZoneName};
use beryl_scene::input::{Insets, Size};
use beryl_scene::table::{DESKTOP, Scroll, Window, place};
use beryl_scene::testing::ViewBuilder;
use glam::Vec2;

/// The web interface's boxes of the cards of the pay-default recording at its end, seen by the
/// viewer in a 1600 by 900 window, from `data/beryl3d/match/web/pay-default_you_1600x900@2.json`:
/// left, top, width and height, in logical points.
const OPPONENT_ISLAND: [f32; 4] = [143.4, 112.0, 92.9, 109.3];
const OPPONENT_PLAINS: [f32; 4] = [233.2, 112.0, 90.8, 109.3];
const YOUR_UNTAPPED: [f32; 4] = [94.3, 570.0, 109.2, 129.2];
const YOUR_FIRST_TAPPED: [f32; 4] = [235.0, 570.0, 138.3, 92.5];
const YOUR_SECOND_TAPPED: [f32; 4] = [377.9, 570.0, 135.7, 92.5];

#[test]
fn desktop_cards_land_on_the_web_interfaces_boxes() {
    let window = Window {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    };
    let mut builder = ViewBuilder::new();
    let mut land = |side: Side, name: &str, tapped: bool| {
        let card = builder.card(ZoneName::Battlefield, side, name);
        card.types = vec![CardType::Land];
        card.tapped = tapped;
        card.slot
    };
    let expected = [
        (land(Side::Opponent, "Island", false), OPPONENT_ISLAND),
        (land(Side::Opponent, "Plains", false), OPPONENT_PLAINS),
        (land(Side::You, "Wastes", false), YOUR_UNTAPPED),
        (land(Side::You, "Island", true), YOUR_FIRST_TAPPED),
        (land(Side::You, "Plains", true), YOUR_SECOND_TAPPED),
    ];
    let view = builder.build();
    let layout = DESKTOP.for_window(window);
    let camera = layout.fit(window);
    let placement = place(&view, &[], &layout, &camera, &Scroll::default());
    let box_of = |slot: ObjectId| {
        let pose = placement.get(slot).expect("the card is placed").pose;
        let (corners, _) = camera.card_box(&pose);
        let low = corners.iter().copied().fold(Vec2::MAX, Vec2::min);
        let high = corners.iter().copied().fold(Vec2::MIN, Vec2::max);
        (low, high)
    };
    let middle =
        |[left, top, width, height]: [f32; 4]| Vec2::new(left + width / 2.0, top + height / 2.0);
    for (slot, web) in expected {
        let (low, high) = box_of(slot);
        let mut centre = (low + high) / 2.0;
        // The web lines a row's cards up by their tops, and Beryl 3D by their middles, so only
        // a tapped card's place across the row is held to the web's.
        if (high.x - low.x) > (high.y - low.y) {
            centre.y = middle(web).y;
        }
        assert!(
            centre.distance(middle(web)) < 3.0,
            "#{}: centre {centre} against the web's {}",
            slot.0,
            middle(web)
        );
        assert!(
            ((high.y - low.y) - web[3]).abs() < 1.5,
            "#{}: {} points tall against the web's {}",
            slot.0,
            high.y - low.y,
            web[3]
        );
    }
}

/// While a creature blocks, it stands straight across the table from its attacker, as the web
/// interface's `row` (`ui/src/game/battlefield.ts`) stands it, whichever side blocks. Before,
/// a blocker kept its place in its row, so its strike went sideways where the web's goes
/// straight across.
#[test]
fn a_blocker_stands_across_from_its_attacker() {
    let window = Window {
        size: Size {
            width: 1600.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    };
    let layout = DESKTOP.for_window(window);
    let camera = layout.fit(window);
    for defender in [Side::Opponent, Side::You] {
        let attacker_side = if defender == Side::You {
            Side::Opponent
        } else {
            Side::You
        };
        let mut builder = ViewBuilder::new();
        let mut creature = |side: Side, name: &str| {
            let card = builder.card(ZoneName::Battlefield, side, name);
            card.types = vec![CardType::Creature];
            card.slot
        };
        let attackers = [
            creature(attacker_side, "Grizzly Bears"),
            creature(attacker_side, "Hill Giant"),
            creature(attacker_side, "Serra Angel"),
        ];
        let bystander = creature(defender, "Wall of Omens");
        let blocker = creature(defender, "Baneslayer Angel");
        let mut view = builder.build();
        for object in &mut view.objects {
            if attackers.contains(&object.slot) {
                object.tapped = true;
                object.combat = Some(Combat::Attacking {
                    planeswalker: None,
                    target_departed: None,
                    blocked: object.slot == attackers[0],
                });
            } else if object.slot == blocker {
                object.combat = Some(Combat::Blocking {
                    attacker: Some(attackers[0]),
                });
            }
        }
        let placement = place(&view, &[], &layout, &camera, &Scroll::default());
        let screen = |slot: ObjectId| {
            let pose = placement.get(slot).expect("the card is placed").pose;
            let (corners, _) = camera.card_box(&pose);
            let low = corners.iter().copied().fold(Vec2::MAX, Vec2::min);
            let high = corners.iter().copied().fold(Vec2::MIN, Vec2::max);
            (low, high)
        };
        let middle = |slot: ObjectId| {
            let (low, high) = screen(slot);
            f32::midpoint(low.x, high.x)
        };
        let across = (middle(blocker) - middle(attackers[0])).abs();
        assert!(across < 1.0, "{defender:?} blocks {across} points aside");
        let (blocker_low, blocker_high) = screen(blocker);
        let (other_low, other_high) = screen(bystander);
        assert!(
            other_high.x <= blocker_low.x || other_low.x >= blocker_high.x,
            "{defender:?}'s other creature makes room for the blocker"
        );
    }
}
