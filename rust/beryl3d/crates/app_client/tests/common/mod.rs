//! Test fixtures for the client and the scripts it plays.

#[path = "../../../game_probe/tests/common/mod.rs"]
mod game;

pub use game::*;

use app_client::{Client, Config, Input};
use beryl_protocol::protocol::Side;
use beryl_protocol::script::{Act, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use kit_ui::input::{Insets, Size};

/// A client at 1440 by 900 that keeps a record of its scripts. It shows the sample table at the
/// frame at 0, then is given a view whose script plays one beat for each act of `acts`, timed
/// as the session times them, which starts at the frame at 1 ms. Returns the client and when
/// each step of that script begins and ends, in nanoseconds since the client started.
#[allow(
    dead_code,
    reason = "The playback allocation test uses only the game fixtures"
)]
pub fn counting(acts: Vec<Act>) -> (Client, Vec<(u64, u64)>) {
    let mut client = Client::new(Config {
        elements: 256,
        draws: 1024,
        name_bytes: 4096,
    });
    client.handle(Input::Surface {
        size: Size {
            width: 1440.0,
            height: 900.0,
        },
        scale: 2.0,
        safe_area: Insets::default(),
    });
    client.record();
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    let mut after = start.clone();
    let mut script = ScriptBuilder::default();
    for act in acts {
        script.beat([act]);
    }
    let mut script = script.build();
    script.timing = time_script(&script, &Played::of(&start.view));
    let mut table = Played::of(&start.view);
    for step in script.beats.iter().flatten() {
        table.apply(&step.act).expect("the table takes each act");
    }
    for (side, state) in [
        (Side::You, &mut after.view.players.you),
        (Side::Opponent, &mut after.view.players.opponent),
    ] {
        state.life = table.piles(side).life;
        state.mana = table.piles(side).mana;
    }
    after.view.script = Some(script);
    client.handle(Input::Update(Box::new(start)));
    client.frame(0);
    client.handle(Input::Update(Box::new(after)));
    client.frame(MS);
    let record = &client.history()[1];
    let begun = record.start.expect("the script has started");
    let steps = (record.steps.iter())
        .map(|step| (begun + step.from, begun + step.to))
        .collect();
    (client, steps)
}
