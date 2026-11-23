use app_client::Input;
use app_client::driver::{Action, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::prompt::Marked;
use beryl_protocol::protocol::{Side, Status, ZoneName};
use beryl_protocol::script::{Act, Mark, Span, Subject, Table, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use kit_frame::frame::{Frame, Material, Role, StateBits};
use kit_ui::input::Size;

use super::flat_layer::{deciding, prompt};

const HAND: [(&str, &str); 7] = [
    ("Portable Hole", "{W}"),
    ("Snapcaster Mage", "{1}{U}"),
    ("Island", ""),
    ("Thirst for Discovery", "{2}{U}"),
    ("Jace, Wielder of Mysteries", "{1}{U}{U}{U}"),
    ("Plains", ""),
    ("Jace, the Mind Sculptor", "{2}{U}{U}"),
];

#[test]
fn a_crowded_hand_draws_printed_costs_without_floating_symbols() {
    let mut view = ViewBuilder::new();
    let slots = HAND.map(|(name, _)| view.card(ZoneName::Hand, Side::You, name).slot);
    view.script(ScriptBuilder::reset().build());
    let start = view.update();
    let initial = FixtureMessage::game_update(&start);

    let eligible = |index: usize| Subject::Object { slot: slots[index] };
    let marks = [eligible(1), eligible(2)].map(|on| Marked {
        on,
        mark: Mark::Eligible,
    });
    let mut script = ScriptBuilder::default()
        .beat(marks.iter().map(|marked| Act::Mark {
            span: Span::Begin,
            mark: marked.mark,
            on: marked.on,
        }))
        .build();
    script.timing = time_script(&script, &Table::of(&start.view));
    view.script(script);
    let mut marked = view.update();
    let Input::Update(payment) = deciding(prompt("Pay {1}{U}")) else {
        unreachable!("deciding supplies an update");
    };
    let Status::Deciding { mut decision } = payment.view.status else {
        unreachable!("the payment has a decision");
    };
    decision.prompt.marks = marks.to_vec();
    marked.view.status = Status::Deciding { decision };
    let marked = FixtureMessage::game_update(&marked);
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
        page.still();
        let frame = page.frame();
        assert_hand(frame);
        assert!(
            frame.rings.iter().any(|ring| ring.name == "eligible"),
            "the payable card has an outline"
        );
        assert!(
            frame.resolved.iter().enumerate().any(|(index, node)| {
                node.role == Role::Text
                    && frame.name(node.name) == "Pay {1}{U}"
                    && frame.draws.iter().zip(&frame.owners).any(|(draw, &owner)| {
                        owner as usize == index && draw.material == Material::Glyph as u8
                    })
            }),
            "the payment prompt draws its cost"
        );
        for (name, cost) in HAND {
            let face = page.faces().find(name).expect("the card has a face");
            assert_eq!(
                page.faces().faces()[usize::from(face.0)]
                    .text
                    .mana_cost
                    .as_deref()
                    .unwrap_or_default(),
                cost,
                "{name}'s printed cost"
            );
        }

        for name in ["Portable Hole", "Snapcaster Mage"] {
            let locator = format!("card \"{name}\"");
            page.hover(&locator);
            page.still();
            assert_hand(page.frame());
            assert!(
                page.find(&locator).state.has(StateBits::HOVERED),
                "{name} is hovered"
            );
        }
        page.act(&Action::Hold(
            "card \"Snapcaster Mage\"".into(),
            format!("{},{}", size.width / 2.0, size.height / 2.0),
        ));
        assert_hand(page.frame());
        assert!(
            page.find("card \"Snapcaster Mage\"")
                .state
                .has(StateBits::HELD),
            "Snapcaster Mage is held"
        );
    }
}

fn assert_hand(frame: &Frame) {
    let mut cards = 0;
    for (index, node) in frame.resolved.iter().enumerate() {
        if node.place.label() != "hand.yours" {
            continue;
        }
        assert_ne!(
            node.role,
            Role::Text,
            "floating hand text: {}",
            frame.name(node.name)
        );
        if node.role == Role::Card {
            cards += 1;
            assert!(
                node.face.is_some()
                    && frame.draws.iter().zip(&frame.owners).any(|(draw, &owner)| {
                        owner as usize == index && draw.material == Material::Card as u8
                    }),
                "{} draws its printed face",
                frame.name(node.name)
            );
        }
    }
    assert_eq!(cards, HAND.len());
}
