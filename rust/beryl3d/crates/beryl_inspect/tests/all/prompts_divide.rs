//! The prompt shape `Divide` on the shape recording (docs/beryl-3d.md, "The flat layer"): five
//! damage divided among three creatures, a share control over each. The test plays the
//! recording to the prompt, acts as the player did, and lets the driver answer each click with
//! the session's recorded reply, failing the test on any click the player did not make.

use std::str::FromStr;

use app_client::driver::{Duration, Moment, Page};
use beryl_inspect::pages::page;
use beryl_protocol::protocol::Side;
use kit_probe::agent::{Fault, Layout, Rule};
use kit_ui::input::{Rect, Size};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// The recipients, by slot, in the order the session gives their shares: the viewer's
/// Snapcaster Mage (#0) and the opponent's Elite Spellbinder (#259) and Trinket Mage (#263).
/// The prompt opens with one damage on each and two left to give out.
const RECIPIENTS: [u16; 3] = [0, 259, 263];

/// `shape-divide` played from the viewer's seat to its prompt, at rest, with the recorded
/// answer held back, so that the test answers it.
fn at_divide() -> Page {
    let mut page = page("shape-divide", Side::You, Layout::Desktop, SIZE)
        .unwrap_or_else(|why| panic!("{why}"));
    page.set_reply_delay(1, Duration::from_millis(60_000));
    let moment = Moment::from_str("prompt:1").unwrap_or_else(|why| panic!("{why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.step(Duration::from_millis(300));
    page
}

fn centre(rect: Rect) -> (f32, f32) {
    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// The share `slot`'s control shows, read from its text.
fn share(page: &mut Page, slot: u16) -> String {
    let tree = page.tree();
    let line = tree
        .lines()
        .skip_while(|line| !line.contains(&format!("region \"share {slot}\"")))
        .find(|line| line.trim_start().starts_with("text "))
        .unwrap_or_else(|| panic!("no share over #{slot}:\n{tree}"));
    line.trim().split('"').nth(1).unwrap_or_default().to_owned()
}

#[test]
fn a_share_control_stands_centred_over_each_recipient() {
    let mut page = at_divide();
    for slot in RECIPIENTS {
        let control = centre(page.find(&format!("region \"share {slot}\"")).rect);
        let card = centre(page.find(&format!("card #{slot}")).rect);
        assert!(
            (control.0 - card.0).abs() < 1.0 && (control.1 - card.1).abs() < 1.0,
            "the share over #{slot} is centred on it: {control:?}, not {card:?}"
        );
        assert_eq!(share(&mut page, slot), "1", "the least share on #{slot}");
    }
}

#[test]
fn more_on_the_first_recipient_asks_one_more_for_it() {
    let mut page = at_divide();
    // The driver answers the click with the reply recorded for `Click::Number { n: 2 }` on
    // Snapcaster Mage, and fails the test on any other click.
    page.press("region \"share 0\" > button \"More\"");
    page.step(Duration::from_millis(300));
    assert_eq!(share(&mut page, 0), "2", "the reply gives it two");
    page.find("text \"Distribute damage. (1)\"");
}

#[test]
fn the_share_controls_overlap_nothing_and_can_be_pressed() {
    let mut page = at_divide();
    let faults: Vec<Fault> = (page.faults().into_iter())
        .filter(|fault| matches!(fault.rule, Rule::Overlap | Rule::Reachable))
        .collect();
    assert!(faults.is_empty(), "{faults:#?}");
    for slot in RECIPIENTS {
        for button in ["More", "Less"] {
            let found = page.find(&format!("region \"share {slot}\" > button \"{button}\""));
            assert!(found.interactive, "{button} over #{slot} takes a press");
        }
    }
}
