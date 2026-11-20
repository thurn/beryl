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
