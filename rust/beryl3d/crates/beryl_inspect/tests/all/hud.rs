//! The game screen's furniture played from a recording: each player's orb with their life total
//! in it, and the step stops about it, which the interface checks pass at the desktop windows.

use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::{Layout, Rule};
use beryl_scene::driver::{Moment, Page};
use beryl_scene::input::Size;

/// `pay-default` from the viewer's seat in a window of `width` by `height`, at its first prompt.
fn pay_default(width: f32, height: f32) -> Page {
    let mut page = pages::page(
        "pay-default",
        Side::You,
        Layout::Desktop,
        Size { width, height },
    )
    .unwrap_or_else(|why| panic!("{why}"));
    page.seek(&"prompt:1".parse::<Moment>().unwrap()).unwrap();
    page
}

#[test]
fn both_orbs_hold_the_life_totals_of_the_view() {
    let mut page = pay_default(1600.0, 900.0);
    for (orb, total) in [
        ("region \"your orb\"", "player \"You 20\""),
        ("region \"opponent's orb\"", "player \"Opponent 20\""),
    ] {
        let orb = page.find(orb).rect();
        let total = page.find(total).rect();
        let (x, y) = (total.x + total.width / 2.0, total.y + total.height / 2.0);
        assert!(
            (x - (orb.x + orb.width / 2.0)).abs() < 1.0
                && (y - (orb.y + orb.height / 2.0)).abs() < 1.0,
            "{total:?} is not in the middle of {orb:?}"
        );
    }
}

#[test]
fn the_orbs_and_stops_overlap_nothing_and_are_reachable_and_named() {
    for (width, height) in [(1440.0, 900.0), (2560.0, 1080.0)] {
        let mut page = pay_default(width, height);
        page.find("toggle \"Opponent's draw\"");
        let faults: Vec<_> = (page.faults().into_iter())
            .filter(|fault| matches!(fault.rule, Rule::Overlap | Rule::Reachable | Rule::Named))
            .map(|fault| fault.message)
            .collect();
        assert!(faults.is_empty(), "{width}x{height}: {faults:#?}");
    }
}
