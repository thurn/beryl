//! The protocol's TypeScript types, generated with ts-rs into one checked-in file,
//! `ui/src/generated/protocol.ts`. A test fails when that file is stale; running it with
//! `BERYL_UPDATE_TS=1` rewrites the file.

use ts_rs::{Config, TS};

use crate::protocol::{
    Candidate, CandidateView, CardType, Combat, Counter, CounterView, DecisionView, GameEvent,
    Kind, Mana, ObjectView, OutcomeView, PlayerState, PlayerView, StackItem, Step, StopsSetting,
    TargetRef, TurnView, Why, ZoneName,
};
use crate::{AgentView, GameUpdate};

/// Where the generated file lives, from the repository root.
pub const PATH: &str = "ui/src/generated/protocol.ts";

/// The generated file's contents.
pub fn typescript() -> String {
    let cfg = Config::new();
    let decls = [
        GameUpdate::decl(&cfg),
        AgentView::decl(&cfg),
        PlayerView::decl(&cfg),
        TurnView::decl(&cfg),
        PlayerState::decl(&cfg),
        ObjectView::decl(&cfg),
        CounterView::decl(&cfg),
        Combat::decl(&cfg),
        StackItem::decl(&cfg),
        TargetRef::decl(&cfg),
        DecisionView::decl(&cfg),
        CandidateView::decl(&cfg),
        Candidate::decl(&cfg),
        OutcomeView::decl(&cfg),
        StopsSetting::decl(&cfg),
        GameEvent::decl(&cfg),
        Kind::decl(&cfg),
        Step::decl(&cfg),
        ZoneName::decl(&cfg),
        Why::decl(&cfg),
        Counter::decl(&cfg),
        Mana::decl(&cfg),
        CardType::decl(&cfg),
    ];
    let mut out = String::from(
        "// Generated from rust/crates/beryl_session by ts-rs; do not edit. Regenerate with\n\
         // `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test typescript`.\n",
    );
    for decl in decls {
        out.push('\n');
        out.push_str("export ");
        out.push_str(&decl);
        out.push('\n');
    }
    out
}
