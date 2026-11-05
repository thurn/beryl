//! Decisions and their answers (architecture §3, §7.1, decision D6), and how a game ends
//! (§12.2).

use crate::bitboard::Bitboard;
use crate::fixed::Fixed;
use crate::observation::Observation;

/// What `next` returns: a decision to make, or the end of the game. A decision is a flat
/// value with its candidates inline, so it can live in the state and be copied with it.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Decide(Decision),
    Over(Outcome),
}

/// An answer: an index into the decision's candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Choice(pub u8);

/// One factored single pick for one player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Decision {
    pub player: u8,
    pub kind: DecisionKind,
    pub candidates: Candidates,
}

impl Decision {
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.len() == 0
    }
}

/// What is being decided. The closed list grows with the mechanics the cards need.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecisionKind {
    /// A priority action ([`Opt::Pass`] is always the first candidate).
    Priority,
    /// Whether the coin flip's winner plays or draws (CR 103.1).
    PlayOrDraw,
    /// Keep or mulligan (CR 103.5).
    KeepOrMulligan,
    /// A card from the hand to put on the bottom of the library after a mulligan (CR 103.5).
    PutOnBottom,
    /// A card to discard down to the maximum hand size (CR 514.1).
    Discard,
    /// The legendary permanent to keep under the legend rule (CR 704.5j).
    LegendRule,
    /// The next of a player's waiting triggered abilities to put on the stack (CR 603.3b).
    TriggerOrder,
    /// Whether to pay the life an entering permanent asks so that it doesn't enter tapped.
    PayLife,
    /// A creature to attack with, or [`Opt::Done`] (CR 508.1a).
    Attacker,
    /// What the creature just declared as an attacker attacks: the defending player or a
    /// planeswalker they control (CR 508.1b).
    AttackTarget,
    /// A creature to block with, or [`Opt::Done`] (CR 509.1a).
    Blocker,
    /// The attacker the creature just declared as a blocker blocks (CR 509.1a).
    BlockTarget,
    /// A blocker to assign lethal damage from the attacker it blocks, or [`Opt::Done`]: the
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
    /// One mana source to tap or floating mana to spend, under manual payment (§10).
    ManaPayment,
    /// Whether to cast a card a resolving spell or ability allows, and how (CR 608.2g).
    CastOffer,
    /// Yes or no, asked by a resolving spell or ability.
    YesNo,
    /// One object, asked by a resolving spell or ability.
    ChooseObject,
    /// A creature to tap for a crew cost, until the tapped creatures' power is enough
    /// (CR 702.122a).
    Crew,
    /// One more object, as an [`Opt::Object`], or [`Opt::Done`]: a resolving spell or
    /// ability's choice of up to a number of objects, one at a time.
    ChooseUpTo,
    /// An artifact or creature to tap for one generic mana of a waterbend cost, as an
    /// [`Opt::Object`], or [`Opt::Done`] to pay the rest with mana (CR 701.67a).
    Waterbend,
}

/// The candidates of a decision. A `Choice(i)` names the `i`th object in ascending slot order,
/// or the `i`th option.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Candidates {
    Objects(Bitboard),
    Options(Fixed<Opt, 64>),
}

impl Candidates {
    pub fn len(&self) -> usize {
        match self {
            Candidates::Objects(b) => b.count() as usize,
            Candidates::Options(o) => o.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The object a choice names, for object candidates.
    pub fn object(&self, c: Choice) -> Option<u8> {
        match self {
            Candidates::Objects(b) => b.nth(usize::from(c.0)),
            Candidates::Options(_) => None,
        }
    }

    /// The option a choice names, for option candidates.
    pub fn option(&self, c: Choice) -> Option<Opt> {
        match self {
            Candidates::Objects(_) => None,
            Candidates::Options(o) => o.as_slice().get(usize::from(c.0)).copied(),
        }
    }
}

/// One option of a decision whose candidates are not simply objects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Opt {
    /// Pass priority.
    #[default]
    Pass,
    /// Play the land card in this slot.
    PlayLand(u8),
    /// Take the first turn.
    Play,
    /// Let the other player take the first turn.
    Draw,
    Keep,
    Mulligan,
    /// The waiting trigger at this index.
    Trigger(u8),
    Yes,
    No,
    /// Finish a declaration or a division.
    Done,
    /// Declare the creature in this slot as an attacker.
    Attack(u8),
    /// Attack the planeswalker in this slot, or, for [`crate::state::NONE`], the defending
    /// player.
    Defender(u8),
    /// Declare the creature in this slot as a blocker.
    Block(u8),
    /// Assign lethal damage to the blocker in this slot.
    Lethal(u8),
    /// Cast the card in `slot` with face `face` up, paying the casting options in the bitmask
    /// `costs` over the face's [`crate::card::Face::casting`] list.
    Cast {
        slot: u8,
        face: u8,
        costs: u8,
    },
    /// Activate ability `ability` of the object in `slot`'s face up.
    Activate {
        slot: u8,
        ability: u8,
    },
    /// Tap the mana source in `slot` for one mana of the [`crate::card::ManaColor`] `color`.
    Mana {
        slot: u8,
        color: u8,
    },
    /// Spend floating mana of the [`crate::card::ManaColor`] index.
    Pool(u8),
    /// A number: X, or how many targets.
    Number(u8),
    /// The `i`th payment plan.
    Plan(u8),
    /// A player, as a target.
    Player(u8),
    /// The object in this slot, among options that also offer [`Opt::Done`].
    Object(u8),
}

/// How a game ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Win(u8),
    /// A rules draw ([CR 104.4a](https://yawgatog.com/resources/magic-rules/#R1044a)): "If all
    /// the players remaining in a game lose simultaneously, the game is a draw."
    Draw,
    /// The engine gave up: a budget or capacity ran out. Never a rules outcome.
    Stalled(StallReason),
}

/// Which budget or capacity a stalled game exhausted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StallReason {
    DecisionsPerTurn,
    Turns,
    StateBasedIterations,
    /// A fixed-capacity queue or zone was full.
    Capacity,
}

/// Where a card body's mid-resolution choices come from (§3, §7.3).
///
/// An inline source, the rollout policy, answers every question as it is asked, so a body
/// runs once on the committed state and never pauses. Any other source pauses: the body runs
/// on a scratch copy, and a question the choice record cannot answer becomes the next
/// decision.
pub trait Answers {
    /// Whether this source answers every question inline.
    fn inline(&self) -> bool {
        false
    }

    /// Answers `d`, observed by `d.player`. Asked only of an inline source, which must
    /// answer.
    fn answer(&mut self, obs: Observation<'_>, d: &Decision) -> Option<Choice>;
}

/// The answer source for tree nodes and the UI: the engine answers from the choice record in
/// the state, and pauses when it runs out.
pub struct Replay;

impl Answers for Replay {
    fn answer(&mut self, _obs: Observation<'_>, _d: &Decision) -> Option<Choice> {
        None
    }
}
