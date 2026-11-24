//! The kinds of decision: the form of one answer the engine asks for
//! (`docs/engine-architecture.md` §3, §7.1).
//!
//! The decision itself, with its candidates, is the kernel's (`beryl_kernel::decision`, whose
//! `Opt` the comments below name). The kind is here because a scenario names it: the kind of
//! question an answer expects is all a card's scenarios need of a decision, and they compile
//! against this crate alone.

/// What is being decided: the form of one answer. A kind is here when the turn structure or
/// the casting procedure asks it and its answer has a form of its own; a card's or a
/// mechanic's question is one of the general kinds, and the decision's [`Prompt`](crate::Prompt) says what it
/// is for (its [`Purpose`](crate::Purpose)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecisionKind {
    /// A priority action (`Opt::Pass` is always the first candidate).
    Priority,
    /// Whether the coin flip's winner plays or draws (CR 103.1).
    PlayOrDraw,
    /// Keep or mulligan (CR 103.5).
    KeepOrMulligan,
    /// The next of a player's waiting triggered abilities to put on the stack (CR 603.3b).
    TriggerOrder,
    /// A creature to attack with, or `Opt::Done` (CR 508.1a).
    Attacker,
    /// What the creature just declared as an attacker attacks: the defending player or a
    /// planeswalker they control (CR 508.1b).
    AttackTarget,
    /// A creature to block with, or `Opt::Done` (CR 509.1a).
    Blocker,
    /// The attacker the creature just declared as a blocker blocks (CR 509.1a).
    BlockTarget,
    /// A blocker to assign lethal damage from the attacker it blocks, or `Opt::Done`: the
    /// division of a blocked attacker's damage, by which of its blockers it destroys
    /// (CR 510.1c).
    DamageDivision,
    /// The value of X for a spell being cast or an ability being activated (CR 601.2b).
    X,
    /// How many targets to choose for a spec with a variable number (CR 601.2c).
    TargetCount,
    /// One target (CR 601.2c).
    Target,
    /// A payment plan, among plans with different consequences (§10, decision D7).
    PaymentPlan,
    /// One step of a payment the player makes by hand: a mana source to tap or floating mana
    /// to spend, under manual payment (§10), or a permanent to tap in place of one mana of a
    /// cost that allows it, as an `Opt::Object`, with `Opt::Done` to pay the rest with
    /// mana.
    ManaPayment,
    /// Whether to cast a card a resolving spell or ability allows, and how (CR 608.2g).
    CastOffer,
    /// Yes or no, asked by a resolving spell or ability or by a replacement effect.
    YesNo,
    /// One object: asked by a resolving spell or ability, by a cost paid with objects, or by
    /// a rule that has a player choose one (a mulligan's card for the bottom, the cleanup
    /// step's discard, the legend rule's permanent to keep). Each such purpose quotes its
    /// rule ([`Purpose`](crate::Purpose)).
    ChooseObject,
    /// One more object, as an `Opt::Object`, or `Opt::Done`: a resolving spell or
    /// ability's choice of up to a number of objects, one at a time.
    ChooseUpTo,
}
