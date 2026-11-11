//! The open question of a machine step, a spell being cast and a paused resolution
//! (architecture §7.3), with the mutators that open and settle it (the crate's contract,
//! "Mutators"). Its payloads are records: the rules build one and hand it over whole.

use beryl_card::{ManaCost, SpecIx};

use crate::bitboard::Bitboard;
use crate::decision::Decision;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::object::ObjectRef;
use crate::state::State;

/// The step a spell's casting or an ability's activation has reached (CR 601.2, 602.2). The
/// spell or ability is already the top of the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastStep {
    /// Announcing the value of X (CR 601.2b).
    X,
    /// Announcing how many targets to choose for the current spec (CR 601.2c).
    TargetCount,
    /// Choosing the targets of the current spec (CR 601.2c).
    Target,
    /// Choosing among payment plans that differ in their consequences (CR 601.2g, 601.2h).
    Plan,
    /// Paying mana one source at a time, under the `manual_mana` profile.
    Manual,
    /// Tapping creatures for a crew cost one at a time; `left` is the power still needed
    /// (CR 702.122a).
    Crew,
    /// Tapping artifacts and creatures for a waterbend cost's generic mana one at a time;
    /// `left` is the generic mana they may still pay (CR 701.67a).
    Waterbend,
    /// Choosing a card a cost exiles (CR 601.2h): from the hand, as evoke's "Exile a blue card
    /// from your hand", or one of the cards from the graveyard of an additional cost such as
    /// Abhorrent Oculus's "exile six cards from your graveyard". `left` is the number still to
    /// exile. Graveyard cards are chosen in graveyard order, bottom first, each above the one
    /// before, from position `graveyard_from` up, so each set of cards is one sequence of
    /// answers (§7.2).
    Exile,
}

/// A spell being cast or an ability being activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Casting {
    pub player: Seat,
    pub step: CastStep,
    /// The target spec being chosen (CR 601.2c).
    pub spec: SpecIx,
    /// For [`CastStep::Exile`], the graveyard position picks continue from.
    pub graveyard_from: u8,
    /// Targets still to choose for it, or what another step still has to pay.
    pub left: u8,
    /// Generic mana a permission adds to the cost.
    pub extra: u8,
    /// Cast without paying its mana cost
    /// ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189)).
    pub free: bool,
    /// Cast during the resolution of another spell or ability, so no player receives priority
    /// after it is cast ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)).
    pub resolving: bool,
    /// What remains to pay under manual payment.
    pub remaining: ManaCost,
    /// Generic mana paid by tapping artifacts and creatures for a waterbend cost
    /// (CR 701.67a).
    pub bent: u8,
    /// A triggered ability being put on the stack: targets only, no costs and no change of
    /// priority (CR 603.3d).
    pub triggered: bool,
}

/// A paused resolution (architecture §7.3): the answers its body has been given so far, and
/// the question it stopped at. The committed state is still the state before the resolution
/// began, except that it keeps known what the runs of the body showed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Resolving<L: Layout = Small> {
    pub record: L::Queue<L::Index, 32>,
    pub question: Decision<L>,
    /// What each player knew as the resolution began. Every run of the body starts from it,
    /// not from what earlier runs showed, so a run asks its questions with the candidates the
    /// first run asked them with and each recorded index names the answer it named then.
    pub knowledge: [Bitboard<L::Slot>; 2],
}

impl Copy for Resolving<Small> {}

/// The open question when it belongs to a machine step rather than to priority. Flat, like
/// the rest of the state.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pending<L: Layout = Small> {
    /// Priority, or nothing to ask.
    #[default]
    None,
    /// The coin flip's winner chooses who takes the first turn (CR 103.1).
    PlayOrDraw { chooser: Seat },
    /// The London mulligan (CR 103.5). Who is asked follows from the players' statuses.
    Mulligans,
    /// The active player discards down to their maximum hand size (CR 514.1).
    Discard,
    /// The legend rule (CR 704.5j): `kept` holds the permanent each player chose to keep so
    /// far.
    LegendRule { kept: Bitboard<L::Slot> },
    /// A land being played asks whether its controller pays life so that it doesn't enter
    /// tapped ([`beryl_card::Replacement::EntersTappedUnlessPayLife`]).
    EnterPayingLife { slot: L::Slot, life: u8 },
    /// The active player declares attackers (CR 508.1), one creature at a time: `attacker`
    /// is the creature just chosen, whose defender is still to choose, if any.
    DeclareAttackers { attacker: Option<L::Slot> },
    /// The defending player declares blockers (CR 509.1), one creature at a time: `blocker`
    /// is the creature just chosen, whose attacker is still to choose, if any.
    DeclareBlockers { blocker: Option<L::Slot> },
    /// The active player divides blocked attackers' combat damage (CR 510.1c): `done` holds
    /// the attackers divided so far, `lethal` the blockers chosen to be dealt lethal damage.
    CombatDamage {
        done: Bitboard<L::Slot>,
        lethal: Bitboard<L::Slot>,
    },
    /// A spell being cast or an ability being activated.
    Casting(Casting),
    /// A body paused for an answer.
    Resolving(Resolving<L>),
    /// A resolving spell or ability lets `player` cast this card now (CR 608.2g).
    CastOffer {
        object: ObjectRef<L>,
        player: Seat,
        free: bool,
    },
    /// The controller of the face-down permanent in `slot` turns it face up, a special action
    /// (CR 116.2b), and chooses how to pay its mana cost: among payment plans, or under manual
    /// payment one source at a time, `remaining` being what is left to pay.
    TurningFaceUp { slot: L::Slot, remaining: ManaCost },
}

impl Copy for Pending<Small> {}

impl<L: Layout> State<L> {
    /// Opens the question of a machine step, or replaces the one open with its next: a
    /// casting that reached its next step is asked again whole. A resolution pauses through
    /// [`State::pause_resolution`], which keeps what it must remember.
    #[inline]
    pub fn ask(&mut self, question: Pending<L>) {
        debug_assert!(
            !matches!(question, Pending::None | Pending::Resolving(_)),
            "a question to open"
        );
        self.pending = question;
    }

    /// Settles the open question: priority, or nothing, is what is asked next.
    #[inline]
    pub fn settle_question(&mut self) {
        self.pending = Pending::None;
    }

    /// Pauses the resolution of the stack's top at `question`, with the answers its body has
    /// been given so far (architecture §7.3). What each player knew as the resolution began
    /// is kept from its first pause: every run of the body starts from that.
    #[inline]
    pub fn pause_resolution(&mut self, record: L::Queue<L::Index, 32>, question: Decision<L>) {
        let knowledge = match &self.pending {
            Pending::Resolving(r) => r.knowledge,
            _ => self.knowledge,
        };
        self.pending = Pending::Resolving(Resolving {
            record,
            question,
            knowledge,
        });
    }

    /// Records the answer to a paused resolution's question, for its body to run again with.
    ///
    /// # Panics
    ///
    /// If no resolution is paused.
    #[inline]
    pub fn record_answer(&mut self, answer: L::Index) -> Result<(), Full> {
        let Pending::Resolving(r) = &mut self.pending else {
            unreachable!("a paused resolution");
        };
        r.record.push(answer)
    }
}
