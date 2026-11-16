//! The prompt shape `Declare` on the shape recordings (docs/beryl-3d.md, "The flat layer"):
//! an attacker clicked where it lies, then what it attacks, the line the reply draws between
//! them, and the line that follows the pointer from an attacker awaiting its aim. Each test
//! plays the recording to the prompt, acts as the player did, and lets the driver answer each
//! click with the session's recorded reply, failing the test on any click the player did not
//! make.

use std::str::FromStr;

use beryl_inspect::pages::page;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::Layout;
use beryl_scene::agent::checks::KeptArrow;
use beryl_scene::agent::{Fault, Rule};
use beryl_scene::driver::{Duration, Moment, Page};
use beryl_scene::frame::Role;
use beryl_scene::input::Size;
use beryl_scene::light::arrows::AIM_ID;
use glam::Vec2;

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

/// `shape-declare`'s sixth prompt, at its 25th view, declares the viewer's attackers. The
/// opponent controls a planeswalker, The Wandering Emperor (#260), so each attacker is aimed
/// at the opponent or at it, and a line is drawn to it. The player pressed All Attack and took
/// it back, then clicked Elite Spellbinder (#5) and then the planeswalker.
const ATTACK: &str = "prompt:6";

/// `shape-declare` played from the viewer's seat to the attack prompt, at rest, and on for as
/// long as its own marks take to come up, with All Attack pressed and taken back as the player
/// did, so that nothing is declared. The recorded answer is held back, so that the test
/// answers it.
fn at_attack() -> Page {
    let mut page = page("shape-declare", Side::You, Layout::Desktop, SIZE)
        .unwrap_or_else(|why| panic!("{why}"));
    page.set_reply_delay(25, Duration::from_millis(60_000));
    let moment = Moment::from_str(ATTACK).unwrap_or_else(|why| panic!("{ATTACK}: {why:?}"));
    page.seek(&moment).unwrap_or_else(|why| panic!("{why}"));
    page.step(Duration::from_millis(300));
    // The buttons each reply brings fade in before they can be pressed.
    page.press("button \"All Attack\"");
    page.step(Duration::from_millis(300));
    page.press("button \"Cancel Attacks\"");
    page.step(Duration::from_millis(300));
    page
}

/// The ends of the arrows to the pointer in the present frame.
fn aim_ends(page: &mut Page) -> Vec<Vec2> {
    let frame = page.frame();
    (frame.resolved.iter())
        .filter(|element| element.role == Role::Arrow && element.id.0 >= AIM_ID)
        .map(|element| element.corners[3])
        .collect()
}

/// How many attack lines the table draws in the present frame that are not arrows to the
/// pointer.
fn attack_lines(page: &mut Page) -> usize {
    let frame = page.frame();
    (frame.resolved.iter())
        .filter(|element| {
            element.role == Role::Arrow
                && element.id.0 < AIM_ID
                && frame.name(element.name) == "attack"
        })
        .count()
}

#[test]
fn an_attacker_then_a_planeswalker_draws_the_line_the_reply_sends() {
    let mut page = at_attack();
    assert_eq!(attack_lines(&mut page), 0, "nothing is declared yet");
    // The driver answers each click with the reply recorded for `Click::On` that card, and
    // fails the test on any other click.
    page.press("card #5");
    page.step(Duration::from_millis(300));
    page.press("card #260");
    page.step(Duration::from_millis(600));
    assert_eq!(
        attack_lines(&mut page),
        1,
        "the reply pairs the attacker with the planeswalker"
    );
}

#[test]
fn a_line_follows_the_pointer_from_an_attacker_awaiting_its_aim() {
    let mut page = at_attack();
    let first = Vec2::new(400.0, 300.0);
    page.move_to(first);
    page.step(Duration::from_millis(16));
    assert_eq!(
        aim_ends(&mut page),
        Vec::new(),
        "nothing awaits its aim yet"
    );
    page.press("card #5");
    page.step(Duration::from_millis(300));
    page.move_to(first);
    page.step(Duration::from_millis(16));
    let ends = aim_ends(&mut page);
    assert_eq!(ends.len(), 1, "one line from the attacker: {ends:?}");
    assert!(
        ends[0].distance(first) < 1.0,
        "it ends at the pointer, not {ends:?}"
    );
    let second = Vec2::new(1000.0, 250.0);
    page.move_to(second);
    page.step(Duration::from_millis(16));
    let ends = aim_ends(&mut page);
    assert!(
        ends.len() == 1 && ends[0].distance(second) < 1.0,
        "it follows to {second}, not {ends:?}"
    );
    page.press("card #260");
    page.step(Duration::from_millis(16));
    assert_eq!(aim_ends(&mut page), Vec::new(), "the attacker is aimed");
}

#[test]
fn lines_hold_while_the_pointer_rests() {
    let mut page = at_attack();
    page.press("card #5");
    page.step(Duration::from_millis(300));
    page.move_to(Vec2::new(700.0, 350.0));
    // "Lines hold" is the one check this test is about: the others run on every recording.
    let lines = |fault: &Fault| fault.rule != Rule::LinesHold;
    let mut kept: Vec<KeptArrow> = Vec::new();
    page.check_since(&mut kept, lines);
    assert!(!kept.is_empty(), "the line to the pointer is drawn");
    for _ in 0..30 {
        page.step(Duration::from_millis(16));
        assert!(page.check_since(&mut kept, lines), "the line is compared");
    }
}
