//! The marks of a permanent's own record: summoning sickness, a token's coin, the attacker's
//! wedge and the blocker's crescent, and no mark on a stolen permanent.

use app_client::Input;
use app_client::driver::{Page, Recording};
use app_client::{Client, Config};
use beryl_protocol::protocol::{CardType, Combat, ObjectId, Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_probe::story::{Body, Story};
use glam::{Affine3A, Vec3};
use kit_base::tokens::{badge, color};
use kit_frame::frame::{
    Band, Draw, ElementId, Endpoint, Frame, Material, Role, ShapeAnchor, Sprite, Vfx, sprites,
};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, Size, Tier, Window};

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
        // An opaque card's soft edge is a second record of the card material.
        .filter(|draw| draw.material == Material::Card as u8 && !draw.is_edge_only())
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
fn the_sickness_badge_follows_the_card_over_its_text_box() {
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
        assert!((0.76..0.80).contains(&from_top), "{from_top}");
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
fn a_creature_spell_on_the_stack_has_no_sickness_badge() {
    let mut client = client();
    let mut view = ViewBuilder::new();
    let card = view.card(ZoneName::Stack, Side::You, "Serra Angel");
    card.types = vec![CardType::Creature];
    card.summoning_sick = true;
    let frame = show(&mut client, &view, 0);
    assert!(marks(&frame, sprites::MOON).is_empty());
    assert_eq!(moon_backdrops(&frame).count(), 0);
}

/// A creature spell that resolves: it leaves the stack and flies to the battlefield. Its zone is
/// the battlefield from the first frame of the flight, but it is drawn large and moving until it
/// lands, and a sickness badge there reads as a permanent the player could attack with.
#[test]
fn a_resolving_creature_spell_has_no_sickness_badge_until_it_lands() {
    use app_client::driver::Duration;
    use beryl_protocol::GameUpdate;
    use beryl_protocol::ipc::FixtureMessage;
    use beryl_protocol::protocol::{StackItem, Why};
    use beryl_protocol::script::Act;
    use beryl_protocol::testing::timing;

    let mut cast = ViewBuilder::new();
    let spell = cast.card(ZoneName::Stack, Side::You, "Serra Angel");
    spell.types = vec![CardType::Creature];
    spell.summoning_sick = true;
    let slot = spell.slot;
    cast.script(ScriptBuilder::reset().build());
    let before = cast.update();

    let mut view = before.view.clone();
    view.stack.retain(|item| *item != StackItem::spell(slot));
    view.battlefield.push(slot);
    let landed = view
        .objects
        .iter_mut()
        .find(|object| object.slot == slot)
        .expect("the spell is in the view");
    landed.zone = ZoneName::Battlefield;
    let object = landed.clone();
    view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Fly {
                from: ZoneName::Stack,
                why: Why::Resolve,
                object,
                at: None,
                item: None,
                origin: None,
            }])
            .build(),
    );
    view.script.as_mut().expect("a script").timing = vec![timing(420.0)];
    let after = GameUpdate {
        session: before.session,
        view,
    };
    let messages = vec![
        FixtureMessage::game_update(&before),
        FixtureMessage::game_update(&after),
    ];
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        Size {
            width: 1440.0,
            height: 900.0,
        },
    )
    .expect("the views can be read");
    let mut moving = 0;
    let mut badges = 0;
    for _ in 0..300 {
        page.step(Duration::from_millis(10));
        let frame = page.frame().clone();
        let resting = !frame
            .resolved
            .iter()
            .any(|element| element.state.has(kit_frame::frame::StateBits::MOVING));
        let moon = !marks(&frame, sprites::MOON).is_empty();
        if !resting {
            moving += 1;
            assert!(
                !moon,
                "a badge is drawn at {} ms, before the card lands",
                page.now() / MS
            );
        }
        badges += usize::from(moon);
    }
    assert!(moving > 0, "the card flies");
    assert!(badges > 0, "the badge appears once the card has landed");
}

#[test]
fn a_token_wears_a_coin_and_a_stolen_permanent_no_outline() {
    let frame = show(
        &mut client(),
        &creature(Side::You, |card| {
            card.token = true;
            card.owner = Side::Opponent;
        }),
        0,
    );
    assert_eq!(marks(&frame, sprites::COIN).len(), 1);
    // Arena marks no permanent for being controlled by a player who does not own it.
    assert!(marks(&frame, sprites::OUTLINE).is_empty());
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
#[test]
fn opposing_combat_marks_leave_room_for_the_block_connector() {
    const SIZE: Size = Size {
        width: 1728.0,
        height: 1117.0,
    };
    let catalog = app_client::stories::embedded();
    let source = catalog
        .iter()
        .find(|source| source.name == "story/game_play/facing-marks")
        .expect("the combat fixture");
    let Body::Capture { capture, .. } = Story::parse(&source.text).unwrap().body else {
        panic!("the embedded combat story is a capture");
    };
    let data = crate::test_data::data();
    let chevron = data.own.wedge.chevron;
    let mut page = Page::in_window(
        data,
        Recording(capture.messages).into(),
        Side::You,
        Window {
            size: SIZE,
            scale: 2.0,
            ..Window::default()
        },
    )
    .unwrap();
    let line = |frame: &Frame, name, slots: [u32; 2]| {
        let mut arrows = frame
            .resolved
            .iter()
            .enumerate()
            .filter(|(_, element)| element.role == Role::Arrow);
        let (index, arrow) = arrows.next().expect("one combat connector");
        assert!(arrows.next().is_none(), "one combat connector");
        assert_eq!(frame.name(arrow.name), name);
        let Some(ShapeAnchor::Line(ends)) = frame.decoration_of(u32::try_from(index).unwrap())
        else {
            panic!("the connector has two anchors");
        };
        assert_eq!(
            ends.map(|end| end.target),
            slots.map(|slot| Endpoint::Node(ElementId(slot)))
        );
    };

    page.seek(&"view:2+150ms".parse().unwrap()).unwrap();
    let declared = page.frame();
    assert_eq!(marks(declared, sprites::WEDGE)[0].1.tint, color::WEDGE);
    line(declared, "attack", [20, 293]);

    page.seek(&"view:3+150ms".parse().unwrap()).unwrap();
    let blocked = page.frame();
    let wedge = marks(blocked, sprites::WEDGE)[0].1;
    let crescent = marks(blocked, sprites::CRESCENT)[0].1;
    assert_eq!(wedge.tint, color::BLOCK_LINE);
    line(blocked, "block", [294, 20]);

    let y = |draw: &Draw, local: Vec3| {
        let point = transform(draw).transform_point3(local);
        -blocked.view_projection.project_point3(point).y * SIZE.height / 2.0
    };
    // The wedge has transparent room beyond its tip and the crescent opens along +x.
    let tip = Vec3::Y * (0.5 * chevron / (chevron + 2.0 * sprites::WEDGE_ROOM));
    let crown = Vec3::NEG_X * 0.45;
    assert!(y(&crescent, crown) > y(&crescent, Vec3::ZERO) + 1.0);
    let gap = y(&wedge, tip) - y(&crescent, crown);
    assert!(
        gap > 2.0,
        "the opposing marks leave a visible gap: {gap} points"
    );

    page.seek(&"end".parse().unwrap()).unwrap();
    let done = page.frame();
    assert!(marks(done, sprites::WEDGE).is_empty());
    assert!(marks(done, sprites::CRESCENT).is_empty());
    assert!(
        done.resolved
            .iter()
            .all(|element| element.role != Role::Arrow)
    );
}

#[test]
fn enhanced_combat_emits_vfx_sparks_and_ambient_motion_and_steps_down_cleanly() {
    let mut client = client();
    client.handle(Input::Device(DeviceInput::Tier(Tier::Enhanced)));

    // Unblocked attacker: Vfx::Chevron in Band::Glows + ember particles, no Baseline wedge.
    let unblocked = show(&mut client, &creature(Side::You, attacking), 100 * MS);
    assert!(unblocked.ambient_motion);
    assert!(marks(&unblocked, sprites::WEDGE).is_empty());
    let vfx_records: Vec<(usize, Draw)> = unblocked
        .draws
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, draw)| draw.is_vfx())
        .collect();
    assert_eq!(vfx_records.len(), 1);
    assert_eq!(unblocked.band(vfx_records[0].0), Some(Band::Glows));
    assert_eq!(vfx_records[0].1.uv[0], Vfx::Chevron as u16);
    let unblocked_sparks = unblocked.load.particles;
    assert!(
        unblocked_sparks > 0,
        "unblocked attacker emits ember sparks"
    );

    // Blocked attacker: Vfx::ChevronBlocked with fewer sparks.
    let blocked = show(
        &mut client,
        &creature(Side::You, |card| {
            card.combat = Some(Combat::Attacking {
                planeswalker: None,
                target_departed: None,
                blocked: true,
            });
        }),
        200 * MS,
    );
    let blocked_vfx: Vec<Draw> = blocked.draws.iter().copied().filter(Draw::is_vfx).collect();
    assert_eq!(blocked_vfx.len(), 1);
    assert_eq!(blocked_vfx[0].uv[0], Vfx::ChevronBlocked as u16);
    assert!(blocked.load.particles > 0 && blocked.load.particles < unblocked_sparks);

    // Blocker: Vfx::ShieldDome in Band::Glows, no Baseline crescent.
    let defender = show(
        &mut client,
        &creature(Side::Opponent, |card| {
            card.combat = Some(Combat::Blocking {
                attacker: Some(ObjectId(7)),
            });
        }),
        300 * MS,
    );
    assert!(defender.ambient_motion);
    assert!(marks(&defender, sprites::CRESCENT).is_empty());
    let shield_vfx: Vec<Draw> = defender
        .draws
        .iter()
        .copied()
        .filter(Draw::is_vfx)
        .collect();
    assert_eq!(shield_vfx.len(), 1);
    assert_eq!(shield_vfx[0].uv[0], Vfx::ShieldDome as u16);

    // Stepping down to Baseline mid-combat restores Baseline marks and clears ambient motion.
    client.handle(Input::Device(DeviceInput::Tier(Tier::Baseline)));
    let stepped = client.frame(400 * MS).clone();
    assert!(!stepped.ambient_motion);
    assert!(stepped.draws.iter().all(|draw| !draw.is_vfx()));
    assert_eq!(marks(&stepped, sprites::CRESCENT).len(), 1);
}
