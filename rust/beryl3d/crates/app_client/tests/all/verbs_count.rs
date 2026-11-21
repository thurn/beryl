//! The verb `count` (docs/beryl-3d.md, "Verbs" and "Effects"): a life total or a pool of
//! unspent mana changes as its step begins, and the `loss` or `gain` the step begins shows by
//! how much a total changed.

use app_client::Client;
use beryl_protocol::protocol::{ManaPool, Side};
use beryl_protocol::script::{Act, Tally};
use glam::Vec2;
use kit_frame::frame::{Frame, Role};

use crate::common::{MS, counting};

/// A step that sets `player`'s life total to `life`.
fn life(player: Side, life: i32) -> Act {
    Act::Count {
        tally: Tally::Life { player, life },
    }
}

/// A step that sets the viewer's pool to two blue and one green.
fn pool() -> Act {
    Act::Count {
        tally: Tally::Pool {
            player: Side::You,
            mana: ManaPool {
                blue: 2,
                green: 1,
                ..ManaPool::default()
            },
        },
    }
}

/// The frame `client` draws at `now`.
fn at(client: &mut Client, now: u64) -> Frame {
    client.frame(now).clone()
}

/// The index of the first element of `role` named `name`, if the frame holds one.
fn element(frame: &Frame, role: Role, name: &str) -> Option<usize> {
    (frame.resolved.iter())
        .position(|element| element.role == role && frame.name(element.name) == name)
}

/// The index of the life total of the player `name`, which is named for them and its number.
fn player(frame: &Frame, name: &str) -> usize {
    (frame.resolved.iter())
        .position(|element| {
            element.role == Role::Player
                && frame
                    .name(element.name)
                    .strip_prefix(name)
                    .is_some_and(|rest| rest.starts_with(' '))
        })
        .expect("a life total for the player")
}

/// The life total the player `name` shows.
fn total(frame: &Frame, name: &str) -> String {
    let name_and_life = frame.name(frame.resolved[player(frame, name)].name);
    name_and_life[name.len() + 1..].to_owned()
}

/// The top and the bottom of an element's box, in logical points.
fn top_and_bottom(frame: &Frame, index: usize) -> (f32, f32) {
    let corners = frame.resolved[index].corners;
    let min = corners.into_iter().reduce(Vec2::min).expect("four corners");
    let max = corners.into_iter().reduce(Vec2::max).expect("four corners");
    (min.y, max.y)
}

/// Life changes end to the left of the total, leaving the Combat notice below it clear.
fn assert_beside_total(frame: &Frame, change: usize, player: usize) {
    let right = frame.resolved[change]
        .corners
        .iter()
        .map(|c| c.x)
        .fold(f32::NEG_INFINITY, f32::max);
    let left = frame.resolved[player]
        .corners
        .iter()
        .map(|c| c.x)
        .fold(f32::INFINITY, f32::min);
    assert!(right < left, "the change stands beside the total");
}

#[test]
fn a_life_total_falls_as_its_step_begins_and_the_loss_sinks_away() {
    // The first beat fills the viewer's pool, and the second takes their life from 20 to 17.
    let (mut client, steps) = counting(vec![pool(), life(Side::You, 17)]);
    let (from, to) = steps[1];
    assert!(from > steps[0].0, "the life step plays in the second beat");
    for now in [MS, from - 1] {
        let frame = at(&mut client, now);
        assert_eq!(total(&frame, "You"), "20", "at {now}");
        assert_eq!(total(&frame, "Opponent"), "20");
        assert!(element(&frame, Role::Text, "-3").is_none());
    }
    let mut last = None;
    for now in [
        from,
        from + (to - from) / 3,
        from + 2 * (to - from) / 3,
        to - 1,
    ] {
        let frame = at(&mut client, now);
        assert_eq!(total(&frame, "You"), "17", "at {now}");
        let player = player(&frame, "You");
        let change = element(&frame, Role::Text, "-3").expect("the loss shows -3");
        let (top, _) = top_and_bottom(&frame, change);
        assert_beside_total(&frame, change, player);
        if let Some(last) = last {
            assert!(top > last, "the loss sinks: {top} after {last}");
        }
        last = Some(top);
    }
    let frame = at(&mut client, to);
    assert_eq!(total(&frame, "You"), "17");
    assert!(
        element(&frame, Role::Text, "-3").is_none(),
        "the loss has ended"
    );
}

#[test]
fn a_gain_rises_beside_the_total() {
    let (mut client, steps) = counting(vec![life(Side::Opponent, 22)]);
    let (from, to) = steps[0];
    let mut last = None;
    for now in [from, from + (to - from) / 2, to - 1] {
        let frame = at(&mut client, now);
        assert_eq!(total(&frame, "Opponent"), "22");
        assert_eq!(total(&frame, "You"), "20");
        let player = player(&frame, "Opponent");
        let change = element(&frame, Role::Text, "+2").expect("the gain shows +2");
        let (_, bottom) = top_and_bottom(&frame, change);
        assert_beside_total(&frame, change, player);
        if let Some(last) = last {
            assert!(bottom < last, "the gain rises: {bottom} after {last}");
        }
        last = Some(bottom);
    }
    assert!(element(&at(&mut client, to), Role::Text, "+2").is_none());
}

#[test]
fn a_pool_is_shown_only_while_it_holds_mana() {
    let (mut client, steps) = counting(vec![pool(), life(Side::You, 17)]);
    let frame = at(&mut client, steps[0].0 - 1);
    assert!(
        element(&frame, Role::Region, "{U}2 {G}").is_none(),
        "no mana, no pool"
    );
    let frame = at(&mut client, steps[0].0);
    assert!(
        element(&frame, Role::Region, "{U}2 {G}").is_some(),
        "the pool as it fills"
    );
}

#[test]
fn no_frame_of_a_loss_lays_anything_out() {
    // The new total and the amount it fell by are laid out as the update arrives, so the frame
    // that begins the loss only shows them.
    let (mut client, steps) = counting(vec![life(Side::You, 17)]);
    let (from, to) = steps[0];
    let mut now = from;
    while now < to {
        let frame = at(&mut client, now);
        assert_eq!(frame.work.layout_nodes, 0, "layout at {now}");
        assert_eq!(frame.work.text_runs, 0, "text shaped at {now}");
        assert_eq!(total(&frame, "You"), "17");
        now += 16 * MS;
    }
}
