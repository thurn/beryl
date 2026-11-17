//! The game screen's furniture played from a recording: each player's orb with their life total
//! in it, the step stops about it, which the interface checks pass at the desktop windows, and the
//! step banner.

use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::{Layout, Rule};
use beryl_scene::driver::{Duration, Moment, Page};
use beryl_scene::input::Size;

/// These moments used to draw life changes, pile counts, row counts or the log through
/// other text. Keep the generic text check on the reported frames, including the Retina
/// window, independently of the recording's allowed faults.
#[test]
fn life_changes_and_pile_counts_leave_other_text_readable() {
    let moments = [
        (
            "adeline-resplendent-cathar/adelines-human-arrives-attacking",
            Side::You,
            "1322ms",
        ),
        (
            "gemrazer/over-bloodghast-the-merged-creature-cant-block",
            Side::Opponent,
            "4500ms",
        ),
        (
            "furnace-of-rath/doubles-damage-and-life-gained-from-griselbrand",
            Side::You,
            "1369ms",
        ),
        (
            "furnace-of-rath/doubles-trample-damage-after-assignment",
            Side::You,
            "930ms",
        ),
        (
            "chains-of-mephistopheles/chains-before-dredge",
            Side::You,
            "4343ms",
        ),
        ("verbs-scry", Side::You, "3316ms"),
        ("combat-attackers", Side::Opponent, "44522ms"),
        ("stress-1024", Side::You, "0ms"),
        ("choice-trigger-order", Side::You, "75704ms"),
        (
            "winds-of-abandon/overloaded-a-basic-per-creature-exiled",
            Side::You,
            "4091ms",
        ),
    ];
    for (recording, side, moment) in moments {
        for window in pages::Windows::Desktop.windows() {
            let Size { width, height } = window.size;
            let mut page = pages::page(recording, side, Layout::Desktop, window.size)
                .unwrap()
                .with_scale(window.scale);
            page.seek(&moment.parse::<Moment>().unwrap()).unwrap();
            let faults: Vec<_> = page
                .faults()
                .into_iter()
                .filter(|fault| fault.rule == Rule::TextOverlap)
                .collect();
            assert!(
                faults.is_empty(),
                "{recording}/{side:?}/{width}x{height} at {moment}: {faults:#?}"
            );
        }
    }
}

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
    use beryl_scene::art::orb::{DESKTOP_SCALE, face_offset};
    let mut page = pay_default(1600.0, 900.0);
    for (side, orb, total) in [
        (Side::You, "region \"your orb\"", "player \"You 20\""),
        (
            Side::Opponent,
            "region \"opponent's orb\"",
            "player \"Opponent 20\"",
        ),
    ] {
        let orb = page.find(orb).rect();
        let total = page.find(total).rect();
        let (x, y) = (total.x + total.width / 2.0, total.y + total.height / 2.0);
        // The total stands in the middle of the orb's core, a few points off its art's middle.
        let core = orb.y + orb.height / 2.0 + face_offset(side) * DESKTOP_SCALE;
        assert!(
            (x - (orb.x + orb.width / 2.0)).abs() < 1.0 && (y - core).abs() < 1.0,
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

/// The step banner after `combat-blockers`' viewer declares their blockers: "Combat" over
/// "Blockers declared", shown from the first frame whose view at rest has the blockers declared
/// for the web's `BANNER_MS`, 2.5 seconds, and gone after.
#[test]
fn the_step_banner_reads_blockers_declared_and_is_gone_two_and_a_half_seconds_later() {
    const TICK: Duration = Duration::from_millis(10);
    let mut page = pages::page(
        "combat-blockers",
        Side::You,
        Layout::Desktop,
        Size {
            width: 1600.0,
            height: 900.0,
        },
    )
    .unwrap_or_else(|why| panic!("{why}"));
    page.set_tick(TICK);
    page.seek(&"view:23".parse::<Moment>().unwrap()).unwrap();
    let detail = "text \"Blockers declared\"";
    let mut steps = 0;
    while page.try_find(detail).is_err() {
        page.step(TICK);
        steps += 1;
        assert!(steps < 500, "no banner reads \"Blockers declared\"");
    }
    let shown = page.now();
    let title = page.find("text \"Combat\"").rect();
    let words = page.find(detail).rect();
    assert!(
        title.y + title.height <= words.y,
        "{title:?} is not over {words:?}"
    );
    page.step(Duration::from_millis(2500 - 20));
    assert!(
        page.try_find(detail).is_ok(),
        "the banner left before 2.5 s"
    );
    page.step(Duration::from_millis(30));
    assert!(
        page.try_find(detail).is_err(),
        "the banner shown at {shown} is still up at {}",
        page.now()
    );
}
