//! The protocol's unit tests.

use super::{candidates::cost_text, mana_text};
use beryl_engine::card::{Cost, ManaCost};
use beryl_engine::state::CounterKind;

#[test]
fn costs_read_as_printed() {
    let mana = |generic, symbols, x| ManaCost {
        generic,
        symbols,
        x,
    };
    // Jace, the Mind Sculptor's -1, Map's cost, Staff of the Storyteller's, kicker {X}.
    let loyalty = Cost {
        loyalty: Some(-1),
        ..Cost::NONE
    };
    assert_eq!(cost_text(&loyalty), "\u{2212}1");
    let map = Cost {
        mana: mana(1, [0; 6], 0),
        tap: true,
        sacrifice_self: true,
        ..Cost::NONE
    };
    assert_eq!(cost_text(&map), "{1}, {T}, Sacrifice this");
    let staff = Cost {
        mana: mana(0, [1, 0, 0, 0, 0, 0], 0),
        tap: true,
        remove: Some(CounterKind::Story),
        ..Cost::NONE
    };
    assert_eq!(cost_text(&staff), "{W}, {T}, Remove a story counter");
    assert_eq!(mana_text(mana(0, [0; 6], 1)), "{X}");
    assert_eq!(mana_text(mana(2, [0, 2, 0, 0, 0, 0], 0)), "{2}{U}{U}");
}
