//! The protocol's unit tests.

use super::{candidates::cost_text, mana_text};
use beryl_engine::card::{Cost, CostPart, ManaCost};
use beryl_engine::state::CounterKind;

#[test]
fn costs_read_as_printed() {
    const STAFF: &[CostPart] = &[CostPart::TAP, CostPart::remove(CounterKind::Story)];
    let mana = |generic, symbols, x| ManaCost {
        generic,
        symbols,
        x,
    };
    // Jace, the Mind Sculptor's -1, Map's cost, Staff of the Storyteller's, kicker {X}.
    let loyalty = Cost::loyalty(-1);
    assert_eq!(cost_text(&loyalty), "\u{2212}1");
    let map = Cost::of(mana(1, [0; 6], 0), &[CostPart::TAP, CostPart::SACRIFICE]);
    assert_eq!(cost_text(&map), "{1}, {T}, Sacrifice this");
    let staff = Cost::of(mana(0, [1, 0, 0, 0, 0, 0], 0), STAFF);
    assert_eq!(cost_text(&staff), "{W}, {T}, Remove a story counter");
    assert_eq!(mana_text(mana(0, [0; 6], 1)), "{X}");
    assert_eq!(mana_text(mana(2, [0, 2, 0, 0, 0, 0], 0)), "{2}{U}{U}");
}

#[test]
fn a_neutral_view_round_trips_without_manual_defaults_on_the_wire() {
    use super::{ObjectId, ObjectView, PlayerState, PlayerView, Side, Status, ZoneName};

    let mut view = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
    view.objects.push(ObjectView::hidden(
        ObjectId(1),
        ZoneName::Sideboard,
        Side::Opponent,
        Side::Opponent,
    ));
    let wire = serde_json::to_value(&view).unwrap();
    assert_eq!(wire["status"]["kind"], "free");
    assert!(wire["turn"]["priority"].is_null());
    assert!(wire.get("table").is_none());
    assert!(wire["players"]["you"].get("namedCounters").is_none());
    let object = &wire["objects"][0];
    for key in ["note", "pinned", "face", "namedCounters", "doesntUntap"] {
        assert!(object.get(key).is_none(), "{key} has a wire default");
    }
    let decoded: PlayerView = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.status, Status::Free);
    assert_eq!(decoded, view);
}
