//! Every script of every recording, from both seats, replays to the table of the view it came
//! with (`beryl_protocol::script::replay::check`), played over the table the script before it
//! ended on, as the client plays it. The session checks each script it builds against its own
//! table in a debug build, but a recording is what a client receives, after whatever made it
//! (a spec moment's stand-in, a stress board) has changed the views.

use beryl_inspect::{pages, recordings};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use beryl_protocol::script::{Table, replay};

#[test]
fn every_script_replays_to_its_view() {
    let mut failures = Vec::new();
    for (name, seat) in recordings::names().expect("the recordings pack is readable") {
        let side = if seat == Seat::P0 {
            Side::You
        } else {
            Side::Opponent
        };
        // A recording that could not be recorded fails the interface tests instead.
        let Ok(recording) = pages::recording(&name, side) else {
            continue;
        };
        let views = recording.views().expect("a recording's views are views");
        // The table the scripts so far leave, with their abilities' numbers. None before the
        // first script that resets.
        let mut followed: Option<Table> = None;
        for (at, update) in views.iter().enumerate() {
            let view = &update.view;
            let Some(script) = &view.script else {
                failures.push(format!("{name} {seat:?} view {at}: the view has no script"));
                break;
            };
            let after = Table::of(view);
            let before = match (followed.take(), &script.reset) {
                (_, Some(_)) => after.clone(),
                (Some(table), None) => table,
                (None, None) => {
                    failures.push(format!("{name} {seat:?} view {at}: no reset came before"));
                    break;
                }
            };
            if let Some(failure) = replay::check(before.clone(), script, &after) {
                failures.push(format!("{name} {seat:?} view {at}: {failure}"));
                break;
            }
            followed = replay::replay(before, script).ok();
        }
    }
    assert!(
        failures.is_empty(),
        "{} recordings have a script that does not replay:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
