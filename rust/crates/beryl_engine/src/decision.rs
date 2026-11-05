//! Decisions and their answers (architecture §3, §7.1, decision D6), and how a game ends
//! (§12.2).

use crate::bitboard::Bitboard;
use crate::fixed::Fixed;
use crate::observation::Observation;

/// What `next` returns: a decision to make, or the end of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Decide(Decision),
    Over(Outcome),
}

/// An answer: an index into the decision's candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Choice(pub u8);

/// One factored single pick for one player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
}

/// The candidates of a decision. A `Choice(i)` names the `i`th object in ascending slot order,
/// or the `i`th option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
pub trait Answers {
    /// Answers `d`, or returns `None` to pause the body until the answer is supplied.
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
