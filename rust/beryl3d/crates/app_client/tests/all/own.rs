//! The marks of a permanent's own record: summoning sickness, a token's coin, a stolen
//! permanent's outline, the attacker's wedge and the blocker's crescent.

use app_client::Input;
use app_client::{Client, Config};
use beryl_protocol::protocol::{CardType, Combat, ObjectId, Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use glam::{Affine3A, Vec3};
use kit_base::tokens::{badge, color};
use kit_frame::frame::{Band, Draw, Frame, Material, Sprite, sprites};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size};

const MS: u64 = 1_000_000;

fn client() -> Client {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 512,
            draws: 2048,
            name_bytes: 16_384,
        },
    );
    client.handle(Input::Device(DeviceInput::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    }));
    client
}

/// A table with one creature of `side`'s, changed by `change`.
fn creature(
    side: Side,
    change: impl FnOnce(&mut beryl_protocol::protocol::ObjectView),
) -> ViewBuilder {
    let mut view = ViewBuilder::new();
    let card = view.card(ZoneName::Battlefield, side, "Serra Angel");
    card.types = vec![CardType::Creature];
    change(card);
    view
}

/// Shows `view` on `client`, as a reset, and returns its frame at `now`.
fn show(client: &mut Client, view: &ViewBuilder, now: u64) -> Frame {
    let mut view = view.clone();
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    client.frame(now).clone()
}

/// The records of `frame` drawn with `sprite`, with their indexes.
fn marks(frame: &Frame, sprite: Sprite) -> Vec<(usize, Draw)> {
    frame
        .draws
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, draw)| draw.material != Material::Card as u8 && draw.uv == sprite.rect)
        .collect()
}

fn transform(draw: &Draw) -> Affine3A {
    let m = draw.model;
    Affine3A::from_cols_array(&[
        m[0], m[4], m[8], m[1], m[5], m[9], m[2], m[6], m[10], m[3], m[7], m[11],
    ])
}

/// The card's record, which a table of one card holds once.
fn card(frame: &Frame) -> Draw {
    let cards: Vec<_> = frame
        .draws
        .iter()
        .filter(|draw| draw.material == Material::Card as u8)
        .collect();
    assert_eq!(cards.len(), 1, "one card is drawn");
    *cards[0]
}

/// The draw of the dark circular backdrop on a summoning-sickness badge.
fn moon_backdrops(frame: &Frame) -> impl Iterator<Item = (usize, &Draw)> {
    frame.draws.iter().enumerate().filter(|(index, draw)| {
        frame.band(*index) == Some(Band::TableMarks)
            && draw.material == Material::Panel as u8
            && draw.tint == badge::COUNTER
    })
}

#[test]
fn the_sickness_badge_follows_the_card_and_leaves_its_text_clear() {
    for (side, tapped) in [(Side::You, false), (Side::Opponent, true)] {
        let mut client = client();
        let sick = show(
            &mut client,
            &creature(side, |card| {
                card.tapped = tapped;
                card.summoning_sick = true;
            }),
            0,
        );
        let moons = marks(&sick, sprites::MOON);
        let backdrops: Vec<_> = moon_backdrops(&sick).collect();
        assert_eq!(moons.len(), 1, "one crescent");
        assert_eq!(backdrops.len(), 1, "one contrast backdrop");
        let (moon_index, moon) = moons[0];
        let (backdrop_index, backdrop) = backdrops[0];
        assert!(
            backdrop_index < moon_index,
            "the crescent draws over its backdrop"
        );
        assert_eq!(moon.tint, color::MOON);

        let to_card = transform(&card(&sick)).inverse();
        let backdrop_on_card = to_card * transform(backdrop);
        let moon_on_card = to_card * transform(&moon);
        let card_size = &crate::test_data::data().card;
        let center = backdrop_on_card.translation;
        let diameter = backdrop_on_card.matrix3.x_axis.length();
        let width_share = diameter / card_size.width;
        assert!((0.28..0.30).contains(&width_share), "{width_share}");
        let from_top = 0.5 - center.y / card_size.height;
        assert!((0.40..0.44).contains(&from_top), "{from_top}");
        assert!(center.x.abs() < 0.01, "centered on the card: {center}");
        assert!(backdrop_on_card.matrix3.x_axis.y.abs() < 0.01);
        let moon_offset = moon_on_card.translation - center;
        assert!(Vec3::from(moon_offset).truncate().length() < 0.01);
        assert!(moon_offset.z > 0.0, "the crescent lies above its backdrop");

        let well = show(
            &mut client,
            &creature(side, |card| card.tapped = tapped),
            MS,
        );
        assert!(marks(&well, sprites::MOON).is_empty());
        assert_eq!(moon_backdrops(&well).count(), 0);
    }
}

#[test]
fn a_token_wears_a_coin_and_a_stolen_permanent_an_outline() {
    let frame = show(
        &mut client(),
        &creature(Side::You, |card| {
            card.token = true;
            card.owner = Side::Opponent;
        }),
        0,
    );
    assert_eq!(marks(&frame, sprites::COIN).len(), 1);
    assert_eq!(marks(&frame, sprites::OUTLINE).len(), 1);
    let plain = show(&mut client(), &creature(Side::You, |_| {}), 0);
    assert!(marks(&plain, sprites::COIN).is_empty());
    assert!(marks(&plain, sprites::OUTLINE).is_empty());
}

fn attacking(card: &mut beryl_protocol::protocol::ObjectView) {
    card.combat = Some(Combat::Attacking {
        planeswalker: None,
        target_departed: None,
        blocked: false,
    });
}

#[test]
fn an_attackers_wedge_lies_past_the_edge_toward_the_defender() {
    for (side, toward) in [(Side::You, 1.0), (Side::Opponent, -1.0)] {
        let frame = show(&mut client(), &creature(side, attacking), 0);
        let wedges = marks(&frame, sprites::WEDGE);
        assert_eq!(wedges.len(), 1, "one wedge for {side:?}'s attacker");
        let card = transform(&card(&frame)).translation;
        let wedge = transform(&wedges[0].1).translation;
        // The viewer sits at -y, so the opponent's side of the table is +y.
        let past = (wedge.y - card.y) * toward;
        assert!(
            past > crate::test_data::data().card.height / 2.0,
            "{side:?}'s wedge is {past} mm toward the defender from the card's middle"
        );
    }
}

#[test]
fn a_tapped_attackers_wedge_is_on_the_side_facing_the_defender() {
    let frame = show(
        &mut client(),
        &creature(Side::You, |card| {
            attacking(card);
            card.tapped = true;
        }),
        0,
    );
    let wedge = marks(&frame, sprites::WEDGE)[0].1;
    let card = transform(&card(&frame)).translation;
    let past = transform(&wedge).translation.y - card.y;
    assert!(past > crate::test_data::data().card.width / 2.0, "{past}");
}

#[test]
fn a_blockers_crescent_is_in_the_tables_band() {
    let frame = show(
        &mut client(),
        &creature(Side::Opponent, |card| {
            card.combat = Some(Combat::Blocking {
                attacker: Some(ObjectId(7)),
            });
        }),
        0,
    );
    let crescents = marks(&frame, sprites::CRESCENT);
    assert_eq!(crescents.len(), 1);
    assert_eq!(frame.band(crescents[0].0), Some(Band::TableMarks));
    let card = transform(&card(&frame)).translation;
    let past = card.y - transform(&crescents[0].1).translation.y;
    assert!(
        past > crate::test_data::data().card.height / 2.0,
        "toward the viewer's attacker: {past}"
    );
}

#[test]
fn the_wedge_fades_in_and_stays_then_goes_with_the_attack() {
    let mut client = client();
    let view = creature(Side::You, attacking);
    show(&mut client, &view, 0);
    let opacity = |frame: &Frame| marks(frame, sprites::WEDGE)[0].1.tint[3];
    let seen: Vec<u8> = [0, 10, 20, 40, 400, 4000]
        .iter()
        .map(|ms| opacity(client.frame(ms * MS)))
        .collect();
    assert!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "rises: {seen:?}"
    );
    assert!(seen[0] < seen[2], "fades in: {seen:?}");
    assert_eq!(seen[3..], [255, 255, 255], "stays whole: {seen:?}");

    let calm = show(&mut client, &creature(Side::You, |_| {}), 5000 * MS);
    assert!(
        marks(&calm, sprites::WEDGE).is_empty(),
        "no wedge once the attack is over"
    );
}
