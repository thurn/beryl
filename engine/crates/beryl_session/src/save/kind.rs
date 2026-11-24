//! The kind of decision an answer in a save was to ([`Kind`]), and the kinds a save before
//! version 10 named that this build has no more ([`KindBeforeV10`]).

use beryl_engine::DecisionKind;
use serde::{Deserialize, Serialize};

/// What is being decided: [`DecisionKind`], under names a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Priority,
    PlayOrDraw,
    KeepOrMulligan,
    TriggerOrder,
    Attacker,
    AttackTarget,
    Blocker,
    BlockTarget,
    DamageDivision,
    X,
    TargetCount,
    Target,
    PaymentPlan,
    ManaPayment,
    CastOffer,
    YesNo,
    ChooseObject,
    ChooseUpTo,
}

impl Kind {
    /// Every kind, in order. A new kind goes here as well as in the match below, which the
    /// compiler holds to every [`DecisionKind`].
    pub const ALL: [Kind; 18] = [
        Kind::Priority,
        Kind::PlayOrDraw,
        Kind::KeepOrMulligan,
        Kind::TriggerOrder,
        Kind::Attacker,
        Kind::AttackTarget,
        Kind::Blocker,
        Kind::BlockTarget,
        Kind::DamageDivision,
        Kind::X,
        Kind::TargetCount,
        Kind::Target,
        Kind::PaymentPlan,
        Kind::ManaPayment,
        Kind::CastOffer,
        Kind::YesNo,
        Kind::ChooseObject,
        Kind::ChooseUpTo,
    ];
}

impl From<DecisionKind> for Kind {
    fn from(kind: DecisionKind) -> Kind {
        // Exhaustive, so a new kind of decision can't go unnamed.
        match kind {
            DecisionKind::Priority => Kind::Priority,
            DecisionKind::PlayOrDraw => Kind::PlayOrDraw,
            DecisionKind::KeepOrMulligan => Kind::KeepOrMulligan,
            DecisionKind::TriggerOrder => Kind::TriggerOrder,
            DecisionKind::Attacker => Kind::Attacker,
            DecisionKind::AttackTarget => Kind::AttackTarget,
            DecisionKind::Blocker => Kind::Blocker,
            DecisionKind::BlockTarget => Kind::BlockTarget,
            DecisionKind::DamageDivision => Kind::DamageDivision,
            DecisionKind::X => Kind::X,
            DecisionKind::TargetCount => Kind::TargetCount,
            DecisionKind::Target => Kind::Target,
            DecisionKind::PaymentPlan => Kind::PaymentPlan,
            DecisionKind::ManaPayment => Kind::ManaPayment,
            DecisionKind::CastOffer => Kind::CastOffer,
            DecisionKind::YesNo => Kind::YesNo,
            DecisionKind::ChooseObject => Kind::ChooseObject,
            DecisionKind::ChooseUpTo => Kind::ChooseUpTo,
        }
    }
}

/// A kind of decision that saves before version 10 named and this build asks as one of the
/// general kinds, its prompt saying what it is for: a pick for one rule or one cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum KindBeforeV10 {
    PutOnBottom,
    Discard,
    LegendRule,
    PayLife,
    Crew,
    Waterbend,
    ExileCost,
}

impl KindBeforeV10 {
    /// Every kind version 10 dropped.
    pub const ALL: [KindBeforeV10; 7] = [
        KindBeforeV10::PutOnBottom,
        KindBeforeV10::Discard,
        KindBeforeV10::LegendRule,
        KindBeforeV10::PayLife,
        KindBeforeV10::Crew,
        KindBeforeV10::Waterbend,
        KindBeforeV10::ExileCost,
    ];

    /// The kind this build asks the same question as.
    pub const fn now(self) -> Kind {
        match self {
            KindBeforeV10::PutOnBottom
            | KindBeforeV10::Discard
            | KindBeforeV10::LegendRule
            | KindBeforeV10::Crew
            | KindBeforeV10::ExileCost => Kind::ChooseObject,
            KindBeforeV10::PayLife => Kind::YesNo,
            KindBeforeV10::Waterbend => Kind::ManaPayment,
        }
    }
}

/// An answer's kind as a file holds it: under a name of this build, or, in a file before
/// version 10, which the migrations before that one still read, under one it dropped.
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum KindFile {
    Now(Kind),
    Before(KindBeforeV10),
}

impl KindFile {
    pub(super) const fn now(self) -> Kind {
        match self {
            KindFile::Now(kind) => kind,
            KindFile::Before(before) => before.now(),
        }
    }

    pub(super) const fn before_v10(self) -> Option<KindBeforeV10> {
        match self {
            KindFile::Now(_) => None,
            KindFile::Before(before) => Some(before),
        }
    }
}
