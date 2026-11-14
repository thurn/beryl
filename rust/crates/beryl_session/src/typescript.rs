//! The protocol's and the IPC surface's TypeScript types, generated with ts-rs into one
//! checked-in file, `ui/src/generated/protocol.ts`. A test fails when that file is stale;
//! running it with `BERYL_UPDATE_TS=1` rewrites the file.

use ts_rs::{Config, TS};

use crate::bugs::{BugReport, BugSaved, PageInfo, Region, UiLogEntry, UiLogRequest};
use crate::card_records::{CardEntry, CardFace, CardImages, CardRecords, CardRecordsRequest};
use crate::ipc::{
    AppInfo, CancelRequest, ChooseRequest, ConcedeRequest, Fixture, FixtureMessage, PassOnRequest,
    Platform, SetStopsRequest, ShareBugRequest, UndoRequest,
};
use crate::journal::Level;
use crate::prompt::{
    ButtonId, Click, ClickReply, ClickRequest, Lies, Marked, ObjectId, Pair, PartName, Pile,
    PlaceName, PromptButton, PromptView, ShapeBody, Share, TapPreview, Words, purpose_decl,
};
use crate::protocol::{
    AbilityClass, AbilityRef, Candidate, CandidateIndex, CandidateView, CardHandling, CardType,
    Combat, Counter, CounterView, Culprit, DecisionId, DecisionView, Declaration, Facing,
    GameEvent, Keyword, Kind, LossReason, Mana, ManaPool, ObjectView, OutcomeView, PaidCost,
    PassTo, Payment, PlayerState, PlayerView, Players, PowerToughness, RefusalView, Refused,
    SessionId, Side, StackItem, Stall, Status, Step, StopsSetting, TargetRef, TurnView, Why,
    ZoneName,
};
use crate::script::{
    Act, Arrow, Fact, Mark, Notice, Reset, Script, Span, Step as ScriptStep, Stream, Subject,
    Tally, Verb,
};
use crate::{AgentView, GameUpdate, SessionError, StartRequest};

/// The prompt's types ([`crate::prompt`]).
fn prompt_decls(cfg: &Config) -> [String; 18] {
    [
        PromptView::decl(cfg),
        purpose_decl(),
        ObjectId::decl(cfg),
        PartName::decl(cfg),
        Lies::decl(cfg),
        Words::decl(cfg),
        Marked::decl(cfg),
        TapPreview::decl(cfg),
        PromptButton::decl(cfg),
        ButtonId::decl(cfg),
        ShapeBody::decl(cfg),
        Pile::decl(cfg),
        Share::decl(cfg),
        Pair::decl(cfg),
        PlaceName::decl(cfg),
        Click::decl(cfg),
        ClickRequest::decl(cfg),
        ClickReply::decl(cfg),
    ]
}

/// Where the generated file lives, from the repository root.
pub const PATH: &str = "ui/src/generated/protocol.ts";

/// The generated file's contents.
pub fn typescript() -> String {
    let cfg = Config::new();
    let decls = [
        AppInfo::decl(&cfg),
        Platform::decl(&cfg),
        Fixture::decl(&cfg),
        FixtureMessage::decl(&cfg),
        StartRequest::decl(&cfg),
        ChooseRequest::decl(&cfg),
        SetStopsRequest::decl(&cfg),
        CancelRequest::decl(&cfg),
        UndoRequest::decl(&cfg),
        PassOnRequest::decl(&cfg),
        ConcedeRequest::decl(&cfg),
        ShareBugRequest::decl(&cfg),
        BugReport::decl(&cfg),
        Region::decl(&cfg),
        PageInfo::decl(&cfg),
        BugSaved::decl(&cfg),
        CardRecordsRequest::decl(&cfg),
        CardRecords::decl(&cfg),
        CardEntry::decl(&cfg),
        CardFace::decl(&cfg),
        CardImages::decl(&cfg),
        UiLogRequest::decl(&cfg),
        UiLogEntry::decl(&cfg),
        Level::decl(&cfg),
        SessionError::decl(&cfg),
        GameUpdate::decl(&cfg),
        AgentView::decl(&cfg),
        SessionId::decl(&cfg),
        DecisionId::decl(&cfg),
        CandidateIndex::decl(&cfg),
        PlayerView::decl(&cfg),
        Status::decl(&cfg),
        CardHandling::decl(&cfg),
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
        PaidCost::decl(&cfg),
        TargetRef::decl(&cfg),
        DecisionView::decl(&cfg),
        PassTo::decl(&cfg),
        Refused::decl(&cfg),
        RefusalView::decl(&cfg),
        CandidateView::decl(&cfg),
        Payment::decl(&cfg),
        Candidate::decl(&cfg),
        OutcomeView::decl(&cfg),
        Stall::decl(&cfg),
        LossReason::decl(&cfg),
        Culprit::decl(&cfg),
        AbilityRef::decl(&cfg),
        AbilityClass::decl(&cfg),
        StopsSetting::decl(&cfg),
        GameEvent::decl(&cfg),
        Kind::decl(&cfg),
        Step::decl(&cfg),
        ZoneName::decl(&cfg),
        Why::decl(&cfg),
        Counter::decl(&cfg),
        Mana::decl(&cfg),
        CardType::decl(&cfg),
        Keyword::decl(&cfg),
        Script::decl(&cfg),
        Reset::decl(&cfg),
        ScriptStep::decl(&cfg),
        Subject::decl(&cfg),
        Act::decl(&cfg),
        Stream::decl(&cfg),
        Tally::decl(&cfg),
        Fact::decl(&cfg),
        Notice::decl(&cfg),
        Span::decl(&cfg),
        Verb::decl(&cfg),
        Mark::decl(&cfg),
        Arrow::decl(&cfg),
    ];
    let mut out = String::from(
        "// Generated from rust/crates/beryl_session by ts-rs; do not edit. Regenerate with\n\
         // `BERYL_UPDATE_TS=1 just test-rust -p beryl_session --test session typescript`.\n",
    );
    for decl in decls.into_iter().chain(prompt_decls(&cfg)) {
        out.push('\n');
        out.push_str("export ");
        out.push_str(&decl);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&crate::ipc::typescript(&cfg));
    out
}
