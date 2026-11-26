use beryl_cli::specs::{View, chosen};

use crate::common::TEST;
use beryl_cli::specs::script::*;

/// Counterspell's first moment, from both seats: every view is numbered, the first shown
/// at once, and the spell's flight to the stack is a step in each seat's script.
#[test]
fn a_moment_prints_each_seats_scripts_view_by_view() {
    let moment = &chosen(&TEST, &["Counterspell".to_owned()]).unwrap()[0].moments()[0];
    for view in View::ALL {
        let text = text(&TEST, moment, view).unwrap();
        assert!(text.starts_with("-- view 1 (shown at once)"), "{text}");
        assert!(text.contains("\n-- view 2 "), "{text}");
        assert!(
            text.lines()
                .any(|l| l.contains("fly ") && l.contains("(Counterspell) hand>stack")),
            "{text}"
        );
    }
}

/// Control Magic's scenario resolves the Aura before the opponent answers it with
/// Metamorphose. The moment's scripts keep that `Resolve`: from both seats Metamorphose is
/// cast only once Control Magic has resolved, and it puts the Aura, not an Island, on top
/// of the library. A permanent put into a library appears there, face up, rather than
/// dissolving in place (`TbrYIs2juCw 1:09:04.55`), and the creature then slides back.
#[test]
fn a_scenarios_resolve_holds_the_response_and_a_tucked_permanent_appears() {
    let spec = &chosen(&TEST, &["Control Magic".to_owned()]).unwrap()[0];
    let moment = spec
        .moments()
        .iter()
        .find(|m| m.name == "the-creature-returns-when-control-magic-leaves")
        .expect("the moment");
    for view in View::ALL {
        let text = text(&TEST, moment, view).unwrap();
        let line = |needle: &str| text.lines().position(|l| l.contains(needle));
        let resolved = line("(Control Magic) stack>battlefield").expect(&text);
        let cast = line("(Metamorphose) hand>stack").expect(&text);
        assert!(resolved < cast, "{text}");
        let tucked = line("appear #8 (Control Magic) battlefield>library").expect(&text);
        assert!(
            !text
                .lines()
                .any(|l| l.contains("vanish") && l.contains(">library")),
            "{text}"
        );
        let back = text
            .lines()
            .skip(tucked)
            .any(|l| l.contains("(Wetland Sambar) battlefield>battlefield"));
        assert!(back, "{text}");
    }
}

/// A spec whose scenario taps Svyelunite Temple, a land with a mana ability that sacrifices
/// it, plays: the session's stand-in once panicked on the Temple's `Activate` option, listed
/// among the mana activations at priority, and the moment was skipped.
#[test]
fn a_land_that_sacrifices_for_mana_does_not_stop_a_spec() {
    let spec = &chosen(&TEST, &["Control Magic".to_owned()]).unwrap()[0];
    let moment = spec
        .moments()
        .iter()
        .find(|m| m.name.starts_with("s-stolen-dandn-is-sacrificed"))
        .expect("the moment with Svyelunite Temple");
    for view in View::ALL {
        text(&TEST, moment, view).unwrap();
    }
}
