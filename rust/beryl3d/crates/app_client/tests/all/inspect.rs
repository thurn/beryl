//! The inspector: the card under the pointer at full size beside the table, as the web
//! interface's `InspectAt` places it (`ui/src/game/Inspector.tsx`), drawn flat from its face.

use app_client::driver::{Duration, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use glam::Vec2;
use kit_frame::frame::{Material, Role, StateBits, TextureArray};
use kit_probe::agent::Rule;
use kit_ui::input::{AssetReady, FaceSize, Rect, Size};

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// A table with no stack: lands and a creature on each side, cards in each hand.
fn table() -> ViewBuilder {
    let mut view = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        for _ in 0..3 {
            view.card(ZoneName::Library, side, "Island");
        }
        view.card(ZoneName::Hand, side, "Counterspell");
        view.card(ZoneName::Hand, side, "Memory Lapse");
        for name in ["Island", "Plains", "Island"] {
            view.card(ZoneName::Battlefield, side, name).types = vec![CardType::Land];
        }
        view.card(ZoneName::Battlefield, side, "Serra Angel").types = vec![CardType::Creature];
        view.card(ZoneName::Battlefield, side, "Pacifism").types = vec![CardType::Enchantment];
    }
    view.script(ScriptBuilder::reset().build());
    view
}

/// A page that plays `view` from the viewer's seat, with the card records of its cards.
fn page(view: &ViewBuilder) -> Page {
    let mut messages = vec![FixtureMessage::game_update(&view.update())];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::REGULAR,
        WINDOW,
    )
    .expect("the view can be read");
    page.still();
    page
}

/// Hovers `card` and draws the frame after, in which the inspector follows the card the
/// pointer found.
fn hover(page: &mut Page, card: &str) {
    page.hover(card);
    page.step(Duration::from_millis(16));
}

/// The inspector's box, if it is shown.
fn inspector(page: &mut Page) -> Option<Rect> {
    let frame = page.frame();
    frame
        .resolved
        .iter()
        .find(|element| {
            element.role == Role::Region && frame.name(element.name).ends_with(", inspected")
        })
        .map(kit_scene::hit::bounds)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// The boxes of both players' life orbs.
fn orbs(page: &mut Page) -> Vec<Rect> {
    let frame = page.frame();
    frame
        .resolved
        .iter()
        .filter(|element| element.role == Role::Player)
        .map(kit_scene::hit::bounds)
        .collect()
}

fn centre_x(rect: Rect) -> f32 {
    rect.x + rect.width / 2.0
}

#[test]
fn a_hovered_card_shows_the_inspector_on_the_side_away_from_it_clear_of_the_orbs() {
    let mut page = page(&table());
    assert_eq!(inspector(&mut page), None);
    let orbs = orbs(&mut page);
    assert!(!orbs.is_empty(), "the table shows the life orbs");
    for card in ["card \"Serra Angel\" #8", "card \"Plains\" #6"] {
        let rest = page.find(card).rect();
        hover(&mut page, card);
        assert!(page.find(card).states().has(StateBits::HOVERED), "{card}");
        let shown = inspector(&mut page).unwrap_or_else(|| panic!("{card} is inspected"));
        let card_left = centre_x(rest) < WINDOW.width / 2.0;
        let inspector_left = centre_x(shown) < WINDOW.width / 2.0;
        assert_ne!(
            card_left, inspector_left,
            "{card} at {rest:?}, inspector {shown:?}"
        );
        assert!((shown.width - 330.0).abs() < 0.5, "{shown:?}");
        for orb in &orbs {
            assert!(
                !overlaps(shown, *orb),
                "{shown:?} covers the orb at {orb:?}"
            );
        }
        page.check();
        let duplicates: Vec<_> = (page.faults().into_iter())
            .filter(|fault| fault.rule == Rule::Duplicate)
            .collect();
        assert!(duplicates.is_empty(), "{duplicates:?}");
    }
}

#[test]
fn with_the_stack_holding_the_right_it_fits_between_the_orbs_and_the_stack() {
    let mut view = table();
    view.card(ZoneName::Stack, Side::Opponent, "Lightning Bolt");
    let mut page = page(&view);
    let orbs = orbs(&mut page);
    let stack = page.find("card \"Lightning Bolt\"").rect();
    hover(&mut page, "card \"Island\" #5");
    let shown = inspector(&mut page).expect("the Island is inspected");
    assert!(shown.width >= 230.0, "{shown:?}");
    for orb in &orbs {
        assert!(
            shown.x >= orb.x + orb.width,
            "{shown:?} right of the orb {orb:?}"
        );
    }
    assert!(
        shown.x + shown.width <= stack.x,
        "{shown:?} left of the stack {stack:?}"
    );
}

#[test]
fn a_hand_card_rises_instead_and_the_card_under_the_inspector_stays_hovered() {
    let mut page = page(&table());
    hover(&mut page, "card \"Counterspell\" #3");
    assert_eq!(inspector(&mut page), None, "a hand card rises to be read");
    let card = "card \"Serra Angel\" #8";
    hover(&mut page, card);
    let shown = inspector(&mut page).expect("the angel is inspected");
    // A battlefield card that lies under the inspector stays hovered when the pointer is there.
    let under = ["card \"Serra Angel\" #18", "card \"Pacifism\" #19"]
        .into_iter()
        .find(|locator| overlaps(page.find(locator).rect(), shown));
    if let Some(under) = under {
        let rect = page.find(under).rect();
        let x = rect.x.max(shown.x) + 2.0;
        let y = rect.y.max(shown.y) + 2.0;
        page.move_to(Vec2::new(x, y));
        page.step(Duration::from_millis(16));
        assert!(page.find(under).states().has(StateBits::HOVERED), "{under}");
    }
}

#[test]
fn it_asks_for_one_full_size_face_at_a_time_and_draws_it_once_it_lands() {
    let mut page = page(&table());
    page.wanted_faces();
    let mut asked = Vec::new();
    for card in [
        "card \"Serra Angel\" #8",
        "card \"Plains\" #6",
        "card \"Serra Angel\" #8",
        "card \"Pacifism\" #9",
    ] {
        page.hover(card);
        let wanted = page.wanted_faces();
        assert!(wanted.len() <= 1, "{wanted:?}");
        asked.extend(wanted);
        page.step(Duration::from_millis(16));
        let wanted = page.wanted_faces();
        assert!(wanted.len() <= 1, "{wanted:?}");
        asked.extend(wanted);
    }
    // The angel is asked for once, though it was hovered twice.
    assert_eq!(asked.len(), 3, "{asked:?}");
    let pacifism = *asked.last().expect("the last card was asked for");
    let full = |page: &mut Page| {
        (page.frame().draws.iter()).any(|draw| {
            draw.material == Material::Card as u8
                && draw.flags & 0b11 == TextureArray::FullFaces as u8
        })
    };
    assert!(
        !full(&mut page),
        "until it lands, it is drawn from its board-size face"
    );
    page.give_face(AssetReady::Face {
        face: pacifism,
        size: FaceSize::Full,
        layer: 5,
    });
    page.step(Duration::from_millis(16));
    assert!(full(&mut page), "drawn from its full-size face");
}

/// The table with `count` cards in the viewer's graveyard, of three names in turn.
fn graveyard(count: usize) -> ViewBuilder {
    let mut view = table();
    for index in 0..count {
        let name = ["Memory Lapse", "Counterspell", "Plains"][index % 3];
        view.card(ZoneName::Graveyard, Side::You, name);
    }
    view
}

const PILE: &str = "pile \"Your graveyard\"";

#[test]
fn escape_closes_the_viewer() {
    let mut page = page(&graveyard(3));
    page.press(PILE);
    page.step(Duration::from_millis(16));
    page.find("text \"3 cards · top first\"");
    page.run("key Escape");
    page.step(Duration::from_millis(1000));
    assert!(page.try_find("list \"cards\"").is_err(), "Escape closed it");
}

#[test]
fn the_revealed_pill_stands_over_the_revealed_pile_and_its_cross_puts_it_away() {
    use beryl_protocol::script::Act;
    use beryl_protocol::testing::timing;
    use kit_frame::frame::REVEALED_FACE;

    let mut view = table();
    let shown = view
        .card(ZoneName::Hand, Side::You, "Lightning Bolt")
        .clone();
    let start = view.update();
    let mut script = ScriptBuilder::default();
    script.beat([Act::Showcase {
        object: shown.clone(),
    }]);
    let mut after = start.clone();
    after.view.script = Some(beryl_protocol::testing::script(
        None,
        script.build().beats,
        vec![timing(300.0)],
    ));
    let messages = vec![
        FixtureMessage::game_update(&start),
        FixtureMessage::game_update(&after),
    ];
    let mut messages = messages;
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::REGULAR,
        WINDOW,
    )
    .expect("the view can be read");
    page.still();
    page.step(Duration::from_millis(1000));
    let face = REVEALED_FACE + u32::from(shown.slot.0);
    let faces = (page.frame().resolved.iter())
        .find(|element| element.id.0 == face)
        .map(kit_scene::hit::bounds)
        .expect("the face lies in the Revealed Cards pile");
    let pill = page.find("button \"Revealed Cards\"").rect();
    assert!(
        pill.y + pill.height <= faces.y && (pill.x - faces.x).abs() < 20.0,
        "the pill {pill:?} stands over the pile {faces:?}"
    );
    page.press("button \"Revealed Cards\"");
    page.step(Duration::from_millis(16));
    page.find(&format!("region \"Lightning Bolt #{}\"", shown.slot.0));
    page.press("button \"close\"");
    page.press("button \"Dismiss\"");
    page.step(Duration::from_millis(16));
    assert!(page.try_find("button \"Revealed Cards\"").is_err());
    assert!(
        !(page.frame().resolved.iter()).any(|element| element.id.0 == face),
        "the pile is put away"
    );
}
