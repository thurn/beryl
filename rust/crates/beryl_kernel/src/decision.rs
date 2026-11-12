//! Decisions and their answers (architecture §3, §7.1, decision D6), and how a game ends
//! (§12.2).

use crate::bitboard::{Bitboard, Slots};
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
use crate::state::State;
use beryl_card::{Castings, FaceIx, ManaColor, ObjectAbility, Prompt};

// The kinds are vocabulary: a scenario names the kind of question it answers, and a card's
// scenarios compile against `beryl_card` alone (`docs/project-50k.md`, Crates).
pub use beryl_card::DecisionKind;

/// What `next` returns: a decision to make, or the end of the game. A decision is a flat
/// value with its candidates inline, so it can live in the state and be copied with it.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step<L: Layout = Small> {
    Decide(Decision<L>),
    Over(Outcome),
}

impl Copy for Step<Small> {}

/// An answer: an index into the decision's candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Choice(pub u16);

/// One factored single pick for one player.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Decision<L: Layout = Small> {
    pub player: Seat,
    pub kind: DecisionKind,
    pub candidates: Candidates<L>,
    /// The prompt this pick belongs to, said once where the rules or a card's code asks it,
    /// so that consumers (the UI, agents) need not recover it from the card asking: what the
    /// question is for, how the player answers it and which part of that this pick is. The
    /// kind says the form of this one answer.
    pub prompt: Prompt,
    /// The objects the prompt is about beyond this pick's candidates, which [`Prompt`], having
    /// no slots, cannot hold. What they are is fixed by the prompt's shape and part
    /// ([`beryl_card::prompt`], "The object set").
    pub about: Bitboard<L::Slot>,
}

impl Copy for Decision<Small> {}

// A decision is copied with the state: the prompt and its object set take the bytes the
// question each card once had its own variant for took, and no more.
const _: () = assert!(size_of::<Decision<Small>>() == 312);

impl<L: Layout> Decision<L> {
    /// A pick of `prompt` among the objects `candidates`.
    pub fn objects(
        player: Seat,
        kind: DecisionKind,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Decision<L> {
        Decision {
            player,
            kind,
            candidates: Candidates::Objects(candidates),
            prompt,
            about: Bitboard::EMPTY,
        }
    }

    /// A pick of `prompt` among the options `list`.
    pub fn options(
        player: Seat,
        kind: DecisionKind,
        list: L::Queue<Opt<L>, 64>,
        prompt: Prompt,
    ) -> Decision<L> {
        Decision {
            player,
            kind,
            candidates: Candidates::Options(list),
            prompt,
            about: Bitboard::EMPTY,
        }
    }

    /// This pick, of a prompt about the objects `about` ([`Decision::about`]).
    #[must_use]
    pub fn about(mut self, about: Bitboard<L::Slot>) -> Decision<L> {
        self.about = about;
        self
    }

    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.len() == 0
    }
}

/// The candidates of a decision. A `Choice(i)` names the `i`th object in ascending slot order,
/// or the `i`th option.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Candidates<L: Layout = Small> {
    Objects(Bitboard<L::Slot>),
    Options(L::Queue<Opt<L>, 64>),
}

impl Copy for Candidates<Small> {}

impl<L: Layout> Candidates<L> {
    pub fn len(&self) -> usize {
        match self {
            Candidates::Objects(b) => b.count() as usize,
            Candidates::Options(o) => o.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The objects offered, in the order a [`Choice`] counts them (the candidate order,
    /// [`State::in_candidate_order`]): none for option candidates.
    #[inline]
    pub fn objects(&self) -> Slots<L::Slot> {
        match self {
            Candidates::Objects(b) => b.iter(),
            Candidates::Options(_) => Bitboard::EMPTY.iter(),
        }
    }

    /// The object a choice names, for object candidates.
    pub fn object(&self, c: Choice) -> Option<L::Slot> {
        match self {
            Candidates::Objects(b) => b.nth(usize::from(c.0)),
            Candidates::Options(_) => None,
        }
    }

    /// The option a choice names, for option candidates.
    pub fn option(&self, c: Choice) -> Option<Opt<L>> {
        match self {
            Candidates::Objects(_) => None,
            Candidates::Options(o) => o.as_slice().get(usize::from(c.0)).copied(),
        }
    }
}

/// One option of a decision whose candidates are not simply objects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Opt<L: Layout = Small> {
    /// Pass priority.
    #[default]
    Pass,
    /// Play the land card in this slot.
    PlayLand(L::Slot),
    /// Take the first turn.
    Play,
    /// Let the other player take the first turn.
    Draw,
    Keep,
    Mulligan,
    /// The waiting trigger at this index.
    Trigger(L::Index),
    Yes,
    No,
    /// Finish a declaration or a division.
    Done,
    /// Declare the creature in this slot as an attacker.
    Attack(L::Slot),
    /// Attack the planeswalker in this slot, or, for `None`, the defending player.
    Defender(Option<L::Slot>),
    /// Declare the creature in this slot as a blocker.
    Block(L::Slot),
    /// Assign lethal damage to the blocker in this slot.
    Lethal(L::Slot),
    /// Cast the card in `slot` with face `face` up, paying the casting options `costs`.
    Cast {
        slot: L::Slot,
        face: FaceIx,
        costs: Castings,
    },
    /// Activate an ability of the object in `slot`.
    Activate {
        slot: L::Slot,
        ability: ObjectAbility,
    },
    /// Tap the mana source in `slot` for one mana of `color`.
    Mana {
        slot: L::Slot,
        color: ManaColor,
    },
    /// Spend floating mana of `color`.
    Pool(ManaColor),
    /// A number: X, or how many targets.
    Number(u8),
    /// The `i`th payment plan.
    Plan(L::Index),
    /// A player, as a target.
    Player(Seat),
    /// The object in this slot, among options that also offer [`Opt::Done`].
    Object(L::Slot),
    /// Turn the face-down permanent in this slot face up, a special action (CR 116.2b).
    TurnFaceUp(L::Slot),
}

const _: () = assert!(size_of::<Opt<Small>>() == 4);

impl<L: Layout> Opt<L> {
    /// The object this option names, if it names one: the card to play, cast, activate or
    /// turn face up, a creature to attack or block with, a blocker, a mana source, a
    /// planeswalker to attack, an object chosen, or a waiting trigger's source.
    pub fn object_named(self, s: &State<L>) -> Option<L::Slot> {
        match self {
            Opt::PlayLand(slot)
            | Opt::Attack(slot)
            | Opt::Block(slot)
            | Opt::Lethal(slot)
            | Opt::Cast { slot, .. }
            | Opt::Activate { slot, .. }
            | Opt::Mana { slot, .. }
            | Opt::Object(slot)
            | Opt::TurnFaceUp(slot) => Some(slot),
            Opt::Defender(slot) => slot,
            Opt::Trigger(i) => s
                .triggers()
                .get(Into::<usize>::into(i))
                .and_then(|tr| tr.source)
                .map(|r| r.slot),
            Opt::Pass
            | Opt::Play
            | Opt::Draw
            | Opt::Keep
            | Opt::Mulligan
            | Opt::Yes
            | Opt::No
            | Opt::Done
            | Opt::Pool(_)
            | Opt::Number(_)
            | Opt::Plan(_)
            | Opt::Player(_) => None,
        }
    }
}

/// How a game ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Win(Seat),
    /// A rules draw ([CR 104.4a](https://yawgatog.com/resources/magic-rules/#R1044a)): "If all
    /// the players remaining in a game lose simultaneously, the game is a draw."
    Draw,
    /// The engine gave up: a budget or capacity ran out. Never a rules outcome.
    Stalled(StallReason),
}

/// Why a player can't cast a card from their hand or play it as a land at priority now
/// (`beryl_engine::refusal`): the first rule its most nearly legal way of playing it breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal<L: Layout = Small> {
    /// Only at sorcery speed, which it is not: an opponent's permanent `by` limits the player
    /// to casting spells at sorcery speed, if the card could be cast now without it.
    Timing { by: Option<L::Slot> },
    /// A land, and the player has played their land this turn.
    LandPlayed,
    /// Its mana cost can't be paid.
    Mana,
    /// Another of its costs, such as cards to exile, can't be paid.
    Cost,
    /// A target it requires can't be chosen.
    Target,
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
pub trait Answers<L: Layout = Small> {
    /// Whether this source answers every question inline.
    fn inline(&self) -> bool {
        false
    }

    /// Answers `d`, observed by `d.player`. Asked only of an inline source, which must
    /// answer.
    fn answer(&mut self, obs: Observation<'_, L>, d: &Decision<L>) -> Option<Choice>;
}

/// The answer source for tree nodes and the UI: the engine answers from the choice record in
/// the state, and pauses when it runs out.
pub struct Replay;

impl<L: Layout> Answers<L> for Replay {
    fn answer(&mut self, _obs: Observation<'_, L>, _d: &Decision<L>) -> Option<Choice> {
        None
    }
}
