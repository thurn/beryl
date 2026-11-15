//! The code behind `beryl_cli`'s commands, as a library so other tools can record the same
//! fixtures and spec moments: [`fixture::record`], [`fixture::definitions`] and
//! [`specs::trace`].

pub mod agreement;
pub mod anchor_replay;
pub mod blunders;
mod boards;
pub mod evalfit;
pub mod fixture;
pub mod freeze;
pub mod game;
pub mod gate;
pub mod hunt;
pub mod ladder;
mod match_log;
mod panics;
pub mod positions;
pub mod review;
pub mod screen;
pub mod script_check;
pub mod serve;
pub mod specs;
pub mod step_timings;
pub mod tournament;
pub mod transcript;
mod walk;
pub mod why;

/// Parses the value of `flag` as a number.
pub fn number<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("{flag}: not a number: {v:?}"))
}
