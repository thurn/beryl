//! Decisions and their answers (architecture §3, §7.1, decision D6), and how a game ends
//! (§12.2).

use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, Castings, FaceIx, ManaColor, ManaCost};
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
use crate::state::{State, Zone};

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
    /// What the question is for, where the kind alone does not say.
    pub ask: Ask<L>,
}

impl Copy for Decision<Small> {}

impl<L: Layout> Decision<L> {
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
    /// A card from the hand to put on the bottom of the library, once a hand is kept after a
    /// mulligan (CR 103.5).
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
    /// A card from the hand to exile to pay a cost, as evoke's "Exile a blue card from your
    /// hand" (CR 601.2h).
    ExileCost,
}

/// What a decision asks, said once where the rules or a card's code asks it, so that
/// consumers (the UI, agents) need not recover it from the card asking. The [`DecisionKind`]
/// says the shape of the answer; this says its meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Ask<L: Layout = Small> {
    /// Nothing more than the kind says.
    #[default]
    Plain,
    /// Creatures whose attack destination is chosen. [CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084):
    /// "their controller chooses which defending player, which planeswalker ... each one is attacking".
    AttackTarget { attackers: Bitboard<L::Slot> },
    /// Scry `count`
    /// ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "look at the top
    /// N cards of your library, then put any number of them on the bottom of your library in
    /// any order and the rest on top of your library in any order." A
    /// [`DecisionKind::ChooseUpTo`] picks a card for the bottom; a
    /// [`DecisionKind::ChooseObject`] the next card for the top.
    Scry { count: u8 },
    /// A look at the cards in `player`'s `zone` (the top `count` of a library, or a whole
    /// hand) that may move one of the `eligible` cards to `to`: a [`DecisionKind::YesNo`]
    /// whether to, then a [`DecisionKind::ChooseObject`] for which.
    Look {
        player: Seat,
        zone: Zone,
        count: u8,
        to: Zone,
        eligible: Bitboard<L::Slot>,
    },
    /// A card from the hand to put on top of the library, `count` of them still to put, each
    /// on the one before.
    PutBack { count: u8 },
    /// Whether to put the top card of `player`'s library on the bottom.
    Fateseal { player: Seat },
    /// A card to put into the hand from the top `count` cards of the library, which the
    /// player has looked at, `take` of them still to put, as Stock Up's "Put two of them into
    /// your hand".
    Take { count: u8, take: u8 },
    /// The next of the top `count` cards of the library, which the player has looked at, to
    /// put on the bottom, under the ones before: Stock Up's "the rest on the bottom of your
    /// library in any order". The last goes unasked.
    Bottom { count: u8 },
    /// Whether to discard a basic land card rather than `count` cards, as Thirst for
    /// Discovery's "discard two cards unless you discard a basic land card".
    DiscardBasicLand { count: u8 },
    /// Whether the card in `card` goes on top of its owner's library (yes) or the bottom
    /// (no), as Subtlety's "Its owner puts it on the top or bottom of their library."
    TopOrBottom { card: L::Slot },
    /// Whether to move the card in `card` to `to`, as Currency Converter's "you may exile
    /// that card from your graveyard"
    /// ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
    /// spell or ability offers any choices ..., the player announces these while applying the
    /// effect."
    MayMove { card: L::Slot, to: Zone },
    /// Whether the card an explore revealed goes to the graveyard
    /// ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): the controller
    /// "may put the revealed card into their graveyard."
    Explore,
    /// A card a library search finds, up to `max` of them, one
    /// [`DecisionKind::ChooseUpTo`] at a time
    /// ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)): "To search for a
    /// card in a zone, look at all cards in that zone (even if it's a hidden zone) and find a
    /// card that matches the given description."
    Search { max: u8 },
    /// Whether to search the library at all, as Trinket Mage's "you may search your library
    /// for ...": declining searches nothing and shuffles nothing, so the answer matters only
    /// to a player who knows where cards in their library are
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
    /// library ..., randomize the cards within it so that no player knows their order."
    MaySearch,
    /// Permanents to untap, up to `max`.
    Untap { max: u8 },
    /// A creature to tap for a crew cost of `crew` power, `left` of it still to tap
    /// ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)): "Tap any number
    /// of untapped creatures you control with total power N or greater".
    Crew { crew: u8, left: u8 },
    /// An artifact or creature to tap for one generic mana of a waterbend cost, `cost` still
    /// to pay ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a)): "you may tap
    /// an untapped artifact or creature you control rather than pay that mana."
    Waterbend { cost: ManaCost },
    /// Whether to pay `cost` so that the effect does not happen
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "[Effect] unless
    /// [player] pays [cost]".
    UnlessPays { cost: ManaCost },
    /// Whether to pay `life` so that the land in `land` doesn't enter tapped.
    PayLife { land: L::Slot, life: u8 },
    /// A card to discard in the cleanup step, `count` of them still to discard to reach the
    /// maximum hand size ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)).
    Cleanup { count: u8 },
    /// Which of the top two cards of the library, which the player has looked at, to manifest
    /// as they manifest dread, the other going to the graveyard
    /// ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)).
    ManifestDread,
    /// A card from `zone`, the hand or the graveyard, to exile to pay a cost, `count` of them
    /// still to exile (CR 601.2h). From a graveyard they are asked for from the bottom up, each
    /// above the one before.
    ExileCards { zone: Zone, count: u8 },
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
        ability: AbilityIx,
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
/// ([`crate::refusal`]): the first rule its most nearly legal way of playing it breaks.
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
