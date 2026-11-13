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
/// spell or ability is already the top of the stack. Each step carries what only it uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastStep<L: Layout = Small> {
    /// Announcing the value of X (CR 601.2b).
    X,
    /// Announcing how many targets to choose for the current spec (CR 601.2c).
    TargetCount,
    /// Choosing the targets of the current spec (CR 601.2c), of which `left` are still to
    /// choose.
    Target { left: u8 },
    /// Choosing among payment plans that differ in their consequences (CR 601.2g, 601.2h).
    Plan,
    /// Paying mana one source at a time, under the `manual_mana` profile: `remaining` is what
    /// is still to pay.
    Manual { remaining: ManaCost },
    /// Paying one part of the cost with objects its player chooses, one at a time.
    Objects(PayingObjects<L>),
}

/// Where [`CastStep::Objects`] has got to in paying one part of the cost with objects its
/// player chooses, one at a time ([CR 601.2h]: "The player pays the total cost.").
///
/// Cards of a graveyard are chosen in graveyard order, bottom first, each above the one
/// before, from position `graveyard_from` up, so each set of cards is one sequence of answers
/// (§7.2).
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PayingObjects<L: Layout = Small> {
    /// The part being paid, counting the cost's parts that are not mana from 0 in the order
    /// printed.
    pub part: u8,
    /// The graveyard position the next pick starts from.
    pub graveyard_from: u8,
    /// The objects chosen so far for a part whose objects all move once the last is chosen:
    /// they stay where they are until then.
    pub chosen: Bitboard<L::Slot>,
    /// What is still to pay, by the part's measure.
    pub left: u8,
}

impl<L: Layout> PayingObjects<L> {
    /// The first question of the part at `part`, with `left` to pay.
    pub fn start(part: u8, left: u8) -> PayingObjects<L> {
        PayingObjects {
            part,
            graveyard_from: 0,
            chosen: Bitboard::EMPTY,
            left,
        }
    }
}

/// What a [`Casting`] puts on the stack, which decides the steps it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastKind {
    /// A spell being cast. `free`: cast without paying its mana cost ([CR 118.9]).
    /// `resolving`: cast during the resolution of another spell or ability, so no player
    /// receives priority after it is cast ([CR 608.2g]).
    ///
    /// [CR 118.9]: https://yawgatog.com/resources/magic-rules/#R1189
    /// [CR 608.2g]: https://yawgatog.com/resources/magic-rules/#R6082g
    Spell { free: bool, resolving: bool },
    /// An activated ability being activated (CR 602.2).
    Activation,
    /// A triggered ability being put on the stack: targets only, no costs and no change of
    /// priority (CR 603.3d).
    Trigger,
}

/// A spell being cast or an ability being activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Casting<L: Layout = Small> {
    pub player: Seat,
    pub kind: CastKind,
    pub step: CastStep<L>,
    /// The target spec being chosen (CR 601.2c).
    pub spec: SpecIx,
    /// Generic mana of the cost that a part paid with objects rather than with mana
    /// ([`beryl_card::Measure::ForGeneric`]). The steps after that part read it.
    pub substituted: u8,
}

impl<L: Layout> Casting<L> {
    /// A casting of `kind` by `player` that starts at `step`, at the first target spec.
    pub fn new(player: Seat, kind: CastKind, step: CastStep<L>) -> Casting<L> {
        Casting {
            player,
            kind,
            step,
            spec: SpecIx::default(),
            substituted: 0,
        }
    }

    /// Whether this is a spell cast without paying its mana cost (CR 118.9).
    pub fn free(&self) -> bool {
        matches!(self.kind, CastKind::Spell { free: true, .. })
    }

    /// Whether this is a spell cast during another's resolution (CR 608.2g).
    pub fn resolving(&self) -> bool {
        matches!(
            self.kind,
            CastKind::Spell {
                resolving: true,
                ..
            }
        )
    }

    /// Whether this puts a triggered ability on the stack (CR 603.3d).
    pub fn triggered(&self) -> bool {
        self.kind == CastKind::Trigger
    }
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
    /// The kernel's own: [`State::pause_resolution`] keeps it and
    /// [`State::resume_resolution`] gives it back, so nothing else decides what a player
    /// knows.
    pub(crate) knowledge: L::PerPlayer<Bitboard<L::Slot>>,
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
    Casting(Casting<L>),
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
            Pending::Resolving(r) => r.knowledge.clone(),
            _ => self.knowledge.clone(),
        };
        self.pending = Pending::Resolving(Resolving {
            record,
            question,
            knowledge,
        });
    }

    /// Settles the open question so that a resolution's body runs from its start. If the
    /// resolution was paused, each player knows again what they knew as it began, which
    /// [`State::pause_resolution`] kept: a run asks its questions with the candidates the
    /// first run asked them with (architecture §7.3). Returns the cards some player knew
    /// beyond that, which the earlier runs revealed and chance must not count as revealed
    /// again ([`crate::chance`]); none for a resolution that was not paused.
    #[inline]
    pub fn resume_resolution(&mut self) -> Bitboard<L::Slot> {
        let revealed = match &self.pending {
            Pending::Resolving(r) => {
                let knowledge = r.knowledge.clone();
                self.restore_knowledge(knowledge)
            }
            _ => Bitboard::EMPTY,
        };
        self.pending = Pending::None;
        revealed
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

#[cfg(test)]
mod tests {
    use beryl_card::Prompt;
    use beryl_card::prompt::Purpose;

    use super::*;
    use crate::decision::{Candidates, DecisionKind};
    use crate::event::Sink;
    use crate::fixed::Fixed;
    use crate::ids::{Slot, SlotId};
    use crate::profile::Profile;

    /// A paused resolution keeps what each player knew at its first pause through every
    /// later one, and resuming it gives exactly that back: a run of the body never starts
    /// knowing what an earlier run showed after a question.
    #[test]
    fn resuming_gives_back_what_was_known_at_the_first_pause() {
        let [known, first, second] = [0, 1, 2].map(Slot::from_index);
        let question = Decision {
            player: Seat::P0,
            kind: DecisionKind::Priority,
            candidates: Candidates::Objects(Bitboard::EMPTY),
            prompt: Prompt::new(Purpose::ALL[0]),
            about: Bitboard::EMPTY,
        };
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.board().set_known(Seat::P0, known, true);
        let began = s.knowledge();

        // Two runs pause, each after a card was shown that the one before had not shown.
        s.pause_resolution(Fixed::default(), question);
        s.look(&mut Sink::default(), Seat::P0, Bitboard::single(first));
        s.pause_resolution(Fixed::default(), question);
        s.look(&mut Sink::default(), Seat::P1, Bitboard::single(second));
        assert!(s.knows(Seat::P0, first) && s.knows(Seat::P1, second));

        let revealed = s.resume_resolution();
        assert_eq!(revealed, [first, second].into_iter().collect());
        assert!(s.knowledge() == began && s.knows(Seat::P0, known));
        assert_eq!(*s.pending(), Pending::None);

        // With no resolution paused there is nothing to give back.
        s.look(&mut Sink::default(), Seat::P0, Bitboard::single(first));
        s.ask(Pending::Mulligans);
        assert!(s.resume_resolution().is_empty());
        assert!(s.knows(Seat::P0, first));
        assert_eq!(*s.pending(), Pending::None);
    }
}
