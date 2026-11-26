//! Descriptions of an unattended game and its outcome for the frame log.

use crate::frame_log::Run;
use crate::options::Options;
use crate::places::{checkout, now_ms};
use beryl_session::protocol::{LossReason, OutcomeView, Side, Stall};

/// How a phone host asks for an unattended game (`BerylConfig::stand_in*`).
pub(crate) struct StandIn {
    pub agent: String,
    pub deck: Option<String>,
    pub opponent: Option<String>,
    pub seed: Option<u64>,
}

/// How `Run::game` describes an unattended game.
pub(super) fn description(
    agent: &str,
    opponent: &str,
    deck: &str,
    seed: u64,
    search_class: beryl_host::Class,
) -> String {
    format!(
        "{agent} against {opponent}, deck {deck}, seed {seed}, search class {}",
        search_class.word()
    )
}

/// What the frame log says of a run `options` describe, stopped at `pause_at`.
pub(super) fn run_of(options: &Options, pause_at: Option<u64>) -> Run {
    Run {
        checkout: checkout(),
        pid: std::process::id(),
        started_ms: now_ms(),
        window: options.window.word().to_owned(),
        display_hz: 0.0,
        adapter: String::new(),
        load: crate::load::averages(),
        fixture: options.fixture.clone(),
        pause_at_ms: pause_at.map(|at| at as f64 / 1e6),
        drive: options.drive.clone(),
        load_threads: options
            .load
            .map(|(threads, class)| format!("{threads}:{}", class.word())),
        slow_gpu_ms: options.slow_gpu.map(|slow| slow.as_secs_f64() * 1e3),
        game: options.agent.as_ref().map(|agent| {
            description(
                agent,
                &options.opponent,
                &options.deck,
                options.seed,
                options.search_class,
            )
        }),
    }
}

/// How `Summary::result` describes a finished game's `OutcomeView`.
pub(super) fn outcome_text(outcome: &OutcomeView) -> String {
    match outcome {
        OutcomeView::Win { winner, reason, by } => {
            let who = match winner {
                Side::You => "You won",
                Side::Opponent => "Opponent won",
            };
            let why = match reason {
                LossReason::Life => "life",
                LossReason::Library => "library",
                LossReason::Effect => "card effect",
                LossReason::Concede => "concession",
            };
            match by {
                Some(culprit) => format!("{who} ({why}, {})", culprit.name),
                None => format!("{who} ({why})"),
            }
        }
        OutcomeView::Draw => "Draw".to_owned(),
        OutcomeView::Stalled { reason } => {
            let why = match reason {
                Stall::DecisionsPerTurn => "too many decisions in one turn",
                Stall::Turns => "too many turns",
                Stall::StateBasedIterations => "state-based actions did not settle",
                Stall::Capacity => "a zone or queue was full",
            };
            format!("Stalled ({why})")
        }
    }
}
