//! The protocol's and the IPC surface's TypeScript types, generated with ts-rs into one
//! checked-in file, `ui/src/generated/protocol.ts`. A test fails when that file is stale;
//! running it with `BERYL_UPDATE_TS=1` rewrites the file.

use ts_rs::{Config, TS};

use crate::ipc::{
    AppInfo, CancelRequest, ChooseRequest, ConcedeRequest, Fixture, FixtureMessage, SetStopsRequest,
};
use crate::protocol::{
    AskView, Candidate, CandidateIndex, CandidateView, CardType, Combat, Counter, CounterView,
    DecisionId, DecisionView, Declaration, Facing, GameEvent, Kind, LossReason, Mana, ManaPool,
    ObjectView, OutcomeView, PassTo, Payment, PlayerState, PlayerView, Players, PowerToughness,
    RefusalView, Refused, SessionId, Side, StackItem, Stall, Status, Step, StopsSetting, TargetRef,
    TurnView, Why, ZoneName,
};
use crate::{AgentView, GameUpdate, SessionError, StartRequest};

/// Where the generated file lives, from the repository root.
pub const PATH: &str = "ui/src/generated/protocol.ts";

/// The generated file's contents.
pub fn typescript() -> String {
    let cfg = Config::new();
    let decls = [
        AppInfo::decl(&cfg),
        Fixture::decl(&cfg),
        FixtureMessage::decl(&cfg),
        StartRequest::decl(&cfg),
        ChooseRequest::decl(&cfg),
        SetStopsRequest::decl(&cfg),
        CancelRequest::decl(&cfg),
        ConcedeRequest::decl(&cfg),
        SessionError::decl(&cfg),
        GameUpdate::decl(&cfg),
        AgentView::decl(&cfg),
        SessionId::decl(&cfg),
        DecisionId::decl(&cfg),
        CandidateIndex::decl(&cfg),
        PlayerView::decl(&cfg),
        Status::decl(&cfg),
        Side::decl(&cfg),
        TurnView::decl(&cfg),
        Players::decl(&cfg),
        PlayerState::decl(&cfg),
        Declaration::decl(&cfg),
        ManaPool::decl(&cfg),
        ObjectView::decl(&cfg),
        Facing::decl(&cfg),
        PowerToughness::decl(&cfg),
        CounterView::decl(&cfg),
        Combat::decl(&cfg),
        StackItem::decl(&cfg),
        TargetRef::decl(&cfg),
        DecisionView::decl(&cfg),
        AskView::decl(&cfg),
        PassTo::decl(&cfg),
        Refused::decl(&cfg),
        RefusalView::decl(&cfg),
        CandidateView::decl(&cfg),
        Payment::decl(&cfg),
        Candidate::decl(&cfg),
        OutcomeView::decl(&cfg),
        Stall::decl(&cfg),
        LossReason::decl(&cfg),
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
    out.push('\n');
    out.push_str(&crate::ipc::typescript(&cfg));
    out
}
