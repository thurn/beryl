//! Contact shadows: one under each card above what lies beneath it, none under a card lying on
//! the table, and each at the height of what lies under it.

use beryl_protocol::GameUpdate;
use beryl_protocol::protocol::{CardType, Side, Why, ZoneName};
use beryl_protocol::script::Act;
use beryl_scene::frame::{Draw, Frame, Material};
use beryl_scene::input::{Input, Insets, Size};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder, object, timing};
use beryl_scene::{Client, Config};

use crate::common::{MS, empty_frame, frame, show};

fn shadows(frame: &Frame) -> Vec<&Draw> {
    frame
        .draws
        .iter()
        .filter(|draw| draw.material == Material::ContactShadow as u8)
        .collect()
}

#[test]
fn every_card_in_a_hand_or_on_the_stack_casts_one_shadow_and_no_card_lying_down_does() {
    let mut builder = ViewBuilder::new();
    for index in 0..7 {
        builder.card(ZoneName::Hand, Side::You, &format!("Mine {index}"));
    }
    for index in 0..5 {
        builder.card(ZoneName::Hand, Side::Opponent, &format!("Theirs {index}"));
    }
    for index in 0..3 {
        builder.card(ZoneName::Stack, Side::You, &format!("Spell {index}"));
    }
    for side in [Side::You, Side::Opponent] {
        for index in 0..10 {
            let types = if index % 2 == 0 {
                vec![CardType::Creature]
            } else {
                vec![CardType::Land]
            };
            builder
                .card(ZoneName::Battlefield, side, &format!("Permanent {index}"))
                .types = types;
        }
        for _ in 0..20 {
            builder.card(ZoneName::Library, side, "Island");
        }
        for _ in 0..3 {
            builder.card(ZoneName::Graveyard, side, "Opt");
        }
        builder.card(ZoneName::Exile, side, "Banished");
    }
    builder.script(ScriptBuilder::reset().build());
    let mut client = Client::new(Config {
        elements: 2048,
        draws: 2048,
        name_bytes: 65_536,
    });
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.handle(Input::Update(Box::new(builder.update())));
    let frame = client.frame(0);
    assert_eq!(shadows(frame).len(), 7 + 5 + 3);
}

#[test]
fn a_card_drawn_off_a_tall_library_casts_its_shadow_on_the_librarys_top() {
    let mut builder = ViewBuilder::new();
    for _ in 0..60 {
        builder.card(ZoneName::Library, Side::You, "Island");
    }
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let top = *start.view.players.you.library.last().expect("a library");
    let mut view = start.view.clone();
    let card = object(top, ZoneName::Hand, Side::You, Some("Island"));
    view.objects.retain(|object| object.slot != top);
    view.objects.push(card.clone());
    view.objects.sort_by_key(|object| object.slot);
    view.players.you.library.retain(|&slot| slot != top);
    view.script = Some(
        ScriptBuilder::default()
            .beat([Act::Fly {
                from: ZoneName::Library,
                why: Why::Draw,
                object: card,
                at: None,
                item: None,
                origin: None,
            }])
            .build(),
    );
    let drawn = GameUpdate {
        session: start.session,
        view,
    };
    let mut show = show();
    show.push(Box::new(start.clone()), Vec::new());
    show.push(Box::new(drawn), vec![timing(400.0)]);
    let mut scratch = empty_frame();
    frame(&mut show, &mut scratch, 0);
    let rests = show.table().placement(&start.view, &[]);
    let pile = rests
        .get(top)
        .expect("the top card is placed")
        .pose
        .position;
    let below = start.view.players.you.library[58];
    let floor = rests.get(below).expect("placed").pose.position.z;
    // The first moment the card has risen clear of the pile while still over it.
    let mut checked = false;
    for ms in 1..400 {
        let now = ms * MS;
        let drawn = frame(&mut show, &mut scratch, now);
        let pose = show
            .pose(usize::from(top.0), now)
            .expect("the card is drawn");
        let over =
            (pose.position.x - pile.x).abs() < 20.0 && (pose.position.y - pile.y).abs() < 30.0;
        if !over || pose.position.z < pile.z + 2.0 {
            continue;
        }
        let shadows = shadows(&drawn);
        assert_eq!(shadows.len(), 1, "at {ms} ms the card casts one shadow");
        let height = shadows[0].model[11];
        assert!(
            height > floor && height < pose.position.z,
            "at {ms} ms the shadow lies at {height}, on the library's top card at {floor}, \
             under the card at {}",
            pose.position.z
        );
        checked = true;
        break;
    }
    assert!(checked, "the card rises clear of the library while over it");
}
