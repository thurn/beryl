//! Every card spec moment opens from the seat it was recorded for and plays to its end.

use app_client::driver::TICK;
use beryl_inspect::pages::{self, Windows};
use beryl_inspect::runner::{caught, each};
use beryl_inspect::{recordings, stress};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use kit_probe::agent::Layout;

/// Card spec moments that the recorder (`beryl_cli`) cannot record, each with the start of the
/// reason the pack gives. A moment listed here that now records fails the test, so that the
/// entry is removed with the fix.
const NOT_RECORDED: &[(&str, &str)] = &[];

/// The most ticks a recording may take to end: a recording that has not ended by then is stuck.
const MOST_TICKS: u32 = 100_000;

/// The card spec moments, as `<card>/<moment>` with the seat each is recorded for. Fixtures
/// and stress boards have no slash in their names.
fn moments() -> Vec<(String, Seat)> {
    recordings::names()
        .expect("the pack opens")
        .into_iter()
        .filter(|(name, _)| name.contains('/') && stress::Board::parse(name).is_none())
        .collect()
}

/// `Ok` when the moment opens and plays to its end, else why not.
fn play(name: &str, seat: Seat) -> Result<(), String> {
    let side = match seat {
        Seat::P0 => Side::You,
        Seat::P1 => Side::Opponent,
    };
    caught(|| {
        let mut page = pages::page(
            name,
            side,
            Layout::Desktop,
            Windows::Desktop.windows()[1].size,
        )?;
        for _ in 0..MOST_TICKS {
            if page.ended() {
                return Ok(());
            }
            page.step(TICK);
        }
        Err(format!("{name}: has not ended after {MOST_TICKS} ticks"))
    })
    .and_then(|played| played)
}

#[test]
fn every_card_spec_moment_opens_and_plays_to_its_end() {
    let moments = moments();
    assert!(
        moments.len() > 1000,
        "only {} card spec moments in the pack",
        moments.len()
    );
    let results = each(&moments, |(name, seat)| play(name, *seat));
    let mut problems = Vec::new();
    for ((name, seat), result) in moments.iter().zip(results) {
        let known = NOT_RECORDED.iter().find(|(known, _)| known == name);
        match (result, known) {
            (Ok(()), Some(_)) => problems.push(format!(
                "{name} ({seat:?}) now plays: remove it from NOT_RECORDED"
            )),
            (Err(why), None) => problems.push(format!("{name} ({seat:?}): {why}")),
            (Err(why), Some((_, reason))) if !why.contains(reason) => problems.push(format!(
                "{name} ({seat:?}) fails for another reason than {reason:?}: {why}"
            )),
            (Ok(()), None) | (Err(_), Some(_)) => {}
        }
    }
    assert!(
        problems.is_empty(),
        "{} moments do not open:\n{}",
        problems.len(),
        problems.join("\n")
    );
}
