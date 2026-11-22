use app_client::driver::{Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{Side, ZoneName};
use beryl_protocol::script::{Act, Mark, Span, Subject, Table, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use kit_frame::frame::{Material, Role, StateBits};
use kit_ui::input::Size;

#[test]
fn hand_costs_paint_over_a_playable_neighbors_outline() {
    let mut view = ViewBuilder::new();
    for name in ["Island", "Plains", "Island", "Trinket Mage"] {
        view.card(ZoneName::Hand, Side::You, name);
    }
    let plains = view.card(ZoneName::Hand, Side::You, "Plains").slot;
    for name in ["Island", "Counterspell", "Island"] {
        view.card(ZoneName::Hand, Side::You, name);
    }
    view.script(ScriptBuilder::reset().build());
    let start = view.update();
    let initial = FixtureMessage::game_update(&start);
    let mut script = ScriptBuilder::default()
        .beat([Act::Mark {
            span: Span::Begin,
            mark: Mark::Eligible,
            on: Subject::Object { slot: plains },
        }])
        .build();
    script.timing = time_script(&script, &Table::of(&start.view));
    view.script(script);
    let marked = FixtureMessage::game_update(&view.update());
    let cards = fixture_message(&[initial.clone(), marked.clone()], TokenMakers::NONE);
    let recording = Recording(vec![cards, initial, marked]);

    for size in [
        Size {
            width: 1728.0,
            height: 1117.0,
        },
        Size {
            width: 393.0,
            height: 759.0,
        },
    ] {
        let mut page = Page::new(
            crate::test_data::data(),
            recording.clone().into(),
            Side::You,
            size,
        )
        .expect("the hand opens")
        .with_scale(2.0);
        page.seek(&"end".parse().unwrap())
            .expect("the marked hand settles");
        page.set_looking(true);
        let frame = page.frame();
        let outline = frame
            .rings
            .iter()
            .find(|ring| ring.name == "eligible")
            .expect("Plains has a playable outline");
        let last_shadow = frame
            .draws
            .iter()
            .zip(&frame.owners)
            .rposition(|(draw, &owner)| {
                owner == outline.element && draw.material == Material::BoxShadow as u8
            })
            .expect("the outline draws a shadow");

        for name in ["{2}{U}", "{U}{U}"] {
            let (index, cost) = frame
                .resolved
                .iter()
                .enumerate()
                .find(|(_, node)| node.role == Role::Text && frame.name(node.name) == name)
                .expect("the mana cost is present");
            assert!(cost.state.has(StateBits::PASSES));
            assert!(!cost.interactive);
            let index = u32::try_from(index).unwrap();
            for material in [Material::Panel, Material::Glyph] {
                let records: Vec<_> = frame
                    .draws
                    .iter()
                    .zip(&frame.owners)
                    .enumerate()
                    .filter(|(_, (draw, owner))| {
                        **owner == index && draw.material == material as u8
                    })
                    .map(|(at, _)| at)
                    .collect();
                assert_eq!(records.len(), 2, "each symbol draws its {material:?}");
                assert!(
                    records.into_iter().all(|at| at > last_shadow),
                    "{name} draws its {material:?} over the playable outline at {size:?}"
                );
            }
        }
    }
}
