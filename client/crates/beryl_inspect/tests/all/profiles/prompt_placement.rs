//! Keep-clear placement on the phone profile.

use beryl_inspect::pages::page;
use beryl_protocol::protocol::Side;
use kit_base::data::profiles::SizeClass;
use kit_ui::input::Size;

#[test]
fn the_phone_target_prompt_is_clear_of_both_creatures_and_the_stack() {
    for (width, height) in [(393.0, 759.0), (375.0, 647.0)] {
        let mut page = page(
            "shape-pick",
            Side::You,
            SizeClass::Compact,
            Size { width, height },
        )
        .unwrap();
        page.seek(&"prompt:6".parse().unwrap()).unwrap();
        let pill = page.find("text \"Choose a target.\"").rect();
        for locator in [
            "card \"Oust\"",
            "card \"Thundertrap Trainer\"",
            "card \"Elite Spellbinder\"",
        ] {
            let card = page.find(locator).rect();
            assert!(
                kit_scene::hit::intersection(pill, card).is_none(),
                "{width}x{height}: the pill {pill:?} covers {locator} {card:?}"
            );
        }
    }
}

#[test]
fn the_phone_waiting_button_leaves_room_for_a_permanent_and_a_stack_ability() {
    let size = Size {
        width: 375.0,
        height: 647.0,
    };
    let mut page = page("shape-mode", Side::Opponent, SizeClass::Compact, size).unwrap();
    page.seek(&"end".parse().unwrap()).unwrap();
    let button = page.find("button \"Opponent's Turn\"").rect();
    for locator in [
        "card \"Currency Converter\"",
        "ability \"Elite Spellbinder\"",
    ] {
        let object = page.find(locator).rect();
        assert!(
            kit_scene::hit::intersection(button, object).is_none(),
            "the waiting button {button:?} covers {locator} {object:?}"
        );
    }
}
