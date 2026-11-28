//! The open question of a machine step, a spell being cast and a paused resolution
//! (architecture §7.3), with the mutators that open and settle it (the crate's contract,
//! "Mutators"). Its payloads are records: the rules build one and hand it over whole.
//!
//! Implements: none. Stores game actions and resolutions that are waiting for a decision.

use beryl_card::{ManaCost, SpecIx, Zone};

use crate::bitboard::Bitboard;
use crate::decision::Decision;
use crate::fixed::Full;
use crate::ids::{Seat, SlotId};
use crate::layout::{Layout, Queue, Small};
use crate::state::State;

/// The step a spell's casting or an ability's activation has reached
/// (see CR 601.2 and see CR 602.2). The spell or ability is already on top of the stack.
/// Each step carries the data it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CastStep<L: Layout = Small> {
    /// Announcing the value of X (see CR 601.2b).
    X,
    /// Announcing how many targets to choose for the current spec (see CR 601.2c).
    TargetCount,
    /// Choosing the targets of the current spec (see CR 601.2c), of which `left` are still to
    /// choose.
    Target { left: u8 },
    /// Choosing among payment plans that differ in their consequences
    /// (see CR 601.2g and see CR 601.2h).
    Plan,
    /// Paying mana one source at a time, under the `manual_mana` profile: `remaining` is what
    /// is still to pay.
    Manual { remaining: ManaCost },
    /// Paying one part of the cost with objects its player chooses, one at a time.
    Objects(PayingObjects<L>),
    /// Activating the outlet mana ability `ability` of the permanent in `slot` in the middle
    /// of the payment (see CR 605.3a): an outlet's cost sacrifices or taps a permanent its
    /// controller chooses. While `paid` is `None` the question is which permanent pays it;
    /// once it is `Some`, which color of mana the ability adds. `remaining` is what is left
    /// to pay under the manual profile ([`CastStep::Manual`]) to return to, and `None` for a
    /// payment by plan, which goes back to choosing a plan.
    Outlet {
        slot: L::Slot,
        ability: beryl_card::ObjectAbility,
        paid: Option<L::Slot>,
        remaining: Option<ManaCost>,
    },
}

/// The next payment decision before a special action can turn a permanent face up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceUpPayment<L: Layout> {
    ChooseCost { morph: beryl_card::Cost },
    Mana(ManaCost),
    Objects(PayingObjects<L>),
}

/// Where [`CastStep::Objects`] has got to in paying one part of the cost with objects its
/// player chooses, one at a time (see [CR 601.2h]: "The player pays the total cost.").
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
    /// A spell being cast. `free`: cast without paying its mana cost (see [CR 118.9]).
    /// `resolving`: cast during the resolution of another spell or ability, so no player
    /// receives priority after it is cast (see [CR 608.2g]).
    ///
    /// [CR 118.9]: https://yawgatog.com/resources/magic-rules/#R1189
    /// [CR 608.2g]: https://yawgatog.com/resources/magic-rules/#R6082g
    Spell { free: bool, resolving: bool },
    /// An activated ability being activated (see CR 602.2).
    Activation,
    /// A triggered ability being put on the stack: targets only, no costs and no change of
    /// priority (see CR 603.3d).
    Trigger,
}

/// A spell being cast or an ability being activated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Casting<L: Layout = Small> {
    pub player: Seat,
    pub kind: CastKind,
    pub step: CastStep<L>,
    /// The target spec being chosen (see CR 601.2c).
    pub spec: SpecIx,
    /// Generic mana of the cost that a part paid with objects rather than with mana
    /// ([`beryl_card::Measure::ForGeneric`]). The steps after that part read it.
    pub substituted: u8,
    /// The colored symbols of the cost that creatures paid for convoke rather than mana
    /// ([`beryl_card::Measure::ForMana`]), three bits a color in W, U, B, R, G order: the
    /// count of color `c` is `(paid_colors >> (3 * c)) & 7`. Generic mana they paid is in
    /// `substituted`. The steps after that part read it ([`Casting::paid`]).
    pub paid_colors: u16,
    /// Where a spell was before it moved to the stack (see CR 601.2a).
    pub from: Zone,
    /// The spell is cast from its owner's graveyard under a permanent's permission to cast
    /// from there once each turn ([`beryl_card::Ability::cast_from_graveyard_each_turn`]):
    /// the value of X is limited to what that permission accepts, and the permission is used
    /// as the spell becomes cast.
    pub from_graveyard_permission: bool,
}

// Preserve the state hash for ordinary casts and activations in saved rollout corpora.
impl<L: Layout> std::hash::Hash for Casting<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        (
            self.player,
            self.kind,
            self.step,
            self.spec,
            self.substituted,
        )
            .hash(h);
        if !matches!(self.from, Zone::Nowhere | Zone::Hand) {
            self.from.hash(h);
        }
        if self.from_graveyard_permission {
            true.hash(h);
        }
        if self.paid_colors != 0 {
            self.paid_colors.hash(h);
        }
    }
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
            paid_colors: 0,
            from: Zone::Nowhere,
            from_graveyard_permission: false,
        }
    }

    /// How many symbols of the color with [`beryl_card::ManaColor`] index `color` (0 to 4)
    /// creatures paid for convoke ([`Casting::paid_colors`]).
    pub fn paid(&self, color: usize) -> u8 {
        u8::try_from((self.paid_colors >> (3 * color)) & 7).expect("three bits")
    }

    /// Records that a creature paid one symbol of the color with index `color` (0 to 4). An
    /// eighth symbol of one color does not fit its three bits: no cost the card table holds
    /// has more than seven colored symbols of one color, so that is `Full`, and the game goes
    /// on in a larger layout, which keeps the same field.
    pub fn pay_color(&mut self, color: usize) -> Result<(), crate::fixed::Full> {
        if self.paid(color) == 7 {
            return Err(crate::fixed::Full);
        }
        self.paid_colors += 1 << (3 * color);
        Ok(())
    }

    /// Whether this is a spell cast without paying its mana cost (see CR 118.9).
    pub fn free(&self) -> bool {
        matches!(self.kind, CastKind::Spell { free: true, .. })
    }

    /// Whether this is a spell cast during another's resolution (see CR 608.2g).
    pub fn resolving(&self) -> bool {
        matches!(
            self.kind,
            CastKind::Spell {
                resolving: true,
                ..
            }
        )
    }

    /// Whether this puts a triggered ability on the stack (see CR 603.3d).
    pub fn triggered(&self) -> bool {
        self.kind == CastKind::Trigger
    }
}

/// A paused resolution (architecture §7.3), or a step's paused turn-based actions
/// ([`Pending::StepActions`]): the answers it has been given so far, and the question it
/// stopped at. The committed state is still the state before the resolution or the actions
/// began, except that it keeps known what the runs showed.
///
/// # Hidden picks
///
/// A pick a player makes without revealing it ([`beryl_card::Effect::Choose`] with `hidden`;
/// see [CR 101.4a]) is recorded as the slot picked ([`hidden_pick_answer`]), not as an index into
/// its candidates: which of a hand's cards a filter accepts is hidden from the other players,
/// so a state determinized for one of them may give the question other candidates, and an
/// index would name another card. `hidden` marks the record's entries that hold such a slot,
/// and determinization deals each slot picked a card the pick's filter accepts
/// ([`crate::observation::Observation::determinize`]).
///
/// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// The record's entries, by position, that answer a hidden pick (bit `i` for entry `i`).
    pub(crate) hidden: u32,
    /// Whether the question is a hidden pick, whose answer [`State::record_answer`] marks as
    /// one.
    pub(crate) asks_hidden: bool,
}

impl Copy for Resolving<Small> {}

/// Every field, but the hidden picks only where there are any: a resolution with none hashes
/// as it did before hidden picks were recorded, so the states the benchmark corpora pin by
/// their hashes still hash the same.
impl<L: Layout> std::hash::Hash for Resolving<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Resolving {
            record,
            question,
            knowledge,
            hidden,
            asks_hidden,
        } = self;
        (record, question, knowledge).hash(h);
        if *hidden != 0 || *asks_hidden {
            (hidden, asks_hidden).hash(h);
        }
    }
}

impl<L: Layout> Resolving<L> {
    /// Whether the question is a hidden pick, answered with the slot picked.
    #[inline]
    pub fn asks_hidden(&self) -> bool {
        self.asks_hidden
    }

    /// The slots the hidden picks recorded so far picked, in the order picked; a pick of
    /// nothing names none.
    pub fn hidden_picks(&self) -> impl Iterator<Item = L::Slot> + '_ {
        self.recorded_answers()
            .filter(|&(_, hidden)| hidden)
            .filter_map(|(answer, _)| hidden_pick_of::<L>(answer))
    }

    /// Each recorded answer and whether it encodes a hidden pick instead of an option index.
    pub(crate) fn recorded_answers(&self) -> impl Iterator<Item = (L::Index, bool)> + '_ {
        let mut hidden = self.hidden;
        self.record.iter().map(move |&answer| {
            let is_hidden = hidden & 1 != 0;
            hidden >>= 1;
            (answer, is_hidden)
        })
    }
}

/// The record's entry for a hidden pick of `slot`, or of nothing ([`Resolving`], "Hidden
/// picks"): 0 for nothing, else one more than the slot's index.
#[inline]
pub fn hidden_pick_answer<L: Layout>(slot: Option<L::Slot>) -> L::Index {
    let n = slot.map_or(0, |slot| slot.index() + 1);
    L::Index::try_from(n)
        .ok()
        .expect("a slot's index and one more fit an index")
}

/// The slot a hidden pick's record entry names ([`hidden_pick_answer`]).
#[inline]
pub fn hidden_pick_of<L: Layout>(answer: L::Index) -> Option<L::Slot> {
    let n: usize = answer.into();
    n.checked_sub(1).map(L::Slot::from_index)
}

/// A player chose to assign lethal combat damage from one creature to another
/// (see CR 510.1c and see CR 510.1d).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LethalAssignment<L: Layout = Small> {
    pub source: L::Slot,
    pub target: L::Slot,
}

/// An exact amount assigned in the current combat damage step. A missing target denotes
/// the player or planeswalker attacked by the source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DamageShare<L: Layout = Small> {
    pub source: L::Slot,
    pub target: Option<L::Slot>,
    pub amount: u8,
}

/// The exact combat damage division while the current source is still assigning.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DamageDivision<L: Layout = Small> {
    pub source: L::Slot,
    pub done: Bitboard<L::Slot>,
    pub recipients: Bitboard<L::Slot>,
    pub remaining: u8,
    pub shares: L::Queue<DamageShare<L>, 32>,
}

impl Copy for DamageDivision<Small> {}

/// The open question when it belongs to a machine step rather than to priority. Flat, like
/// the rest of the state. Its slots are bare, as [`crate::decision::Opt`]'s are: they are
/// written as the question is asked and read as it is answered, in the same state.
///
/// A new variant goes last and takes the next unused tag in the `Hash` impl below, so that
/// every state the benchmark corpora pin by its hash keeps that hash.
#[allow(clippy::large_enum_variant)]
#[repr(u8)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Pending<L: Layout = Small> {
    /// Priority, or nothing to ask.
    #[default]
    None,
    /// The coin flip's winner chooses who takes the first turn (see CR 103.1).
    PlayOrDraw { chooser: Seat },
    /// The London mulligan (see CR 103.5). Who is asked follows from the players' statuses.
    Mulligans,
    /// The active player discards down to their maximum hand size (see CR 514.1).
    Discard,
    /// The legend rule (see CR 704.5j): `kept` holds the permanent each player chose to keep so
    /// far.
    LegendRule { kept: Bitboard<L::Slot> },
    /// A land being played asks whether its controller pays life so that it doesn't enter
    /// tapped ([`beryl_card::Rewrite::EntersTappedUnlessPayLife`]).
    EnterPayingLife { slot: L::Slot, life: u8 },
    /// The active player declares attackers (see CR 508.1), one creature at a time: `attacker`
    /// is the creature just chosen, whose defender is still to choose, if any.
    DeclareAttackers { attacker: Option<L::Slot> },
    /// The defending player declares blockers (see CR 509.1), one creature at a time: `blocker`
    /// is the creature just chosen, whose attacker is still to choose, if any.
    DeclareBlockers { blocker: Option<L::Slot> },
    /// The controller of `source` chooses which opposing creatures take lethal damage
    /// (see CR 510.1c and see CR 510.1d). `done` holds the sources already divided.
    CombatDamage {
        source: L::Slot,
        done: Bitboard<L::Slot>,
        lethal: L::Queue<LethalAssignment<L>, 32>,
    },
    /// A spell being cast or an ability being activated.
    Casting(Casting<L>),
    /// A body paused for an answer.
    Resolving(Resolving<L>),
    /// The turn-based actions of the step that has just begun, paused at a replacement
    /// effect's choice they asked (see [CR 616.1]): a dredge offered for the draw step's draw.
    /// They run again from the start with the answers recorded so far, as a paused
    /// resolution runs its body again.
    ///
    /// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
    StepActions(Resolving<L>),
    /// The controller of the face-down permanent in `slot` turns it face up, a special action
    /// (see CR 116.2b), and chooses which cost to pay or how to pay the chosen cost.
    // Leave the tag of the former post-resolution cast offer unassigned.
    TurningFaceUp {
        slot: L::Slot,
        cost: beryl_card::Cost,
        payment: FaceUpPayment<L>,
    } = 13,
    /// Permanents that entered the battlefield together, whose controllers choose the order
    /// of their timestamps (see [CR 613.7m]): "Objects controlled by the active player ... have an
    /// earlier relative timestamp in the order of that player's choice, followed by each
    /// other player in turn order." `group` holds those still to order, two or more of each
    /// player's; each pick gives the permanent picked the earliest of the timestamps left to
    /// its controller's.
    ///
    /// [CR 613.7m]: https://yawgatog.com/resources/magic-rules/#R6137m
    OrderEntered { group: Bitboard<L::Slot> },
    /// A land being played asks its controller for the value of `kind` it chooses as it
    /// enters ([`beryl_card::Rewrite::Choose`]): playing a land is a special action, so
    /// nothing can pause in the middle of it to ask. `pay_life` is the answer to
    /// [`Pending::EnterPayingLife`], asked first where the land asks both.
    EnterChoosing {
        slot: L::Slot,
        kind: beryl_card::ChosenKind,
        pay_life: bool,
    },
    /// The controller of the permanent in `slot` activates its mana ability `ability`, one
    /// activated only as an instant that makes one mana of any color, and chooses the color
    /// before it is activated.
    ManaColor {
        slot: L::Slot,
        ability: beryl_card::ObjectAbility,
    },
    /// The controller of the permanent in `slot` activates its outlet mana ability `ability`
    /// at priority: while `paid` is `None`, choosing which permanent pays its cost; once
    /// `paid` is `Some`, choosing which color of mana it adds.
    Outlet {
        slot: L::Slot,
        ability: beryl_card::ObjectAbility,
        paid: Option<L::Slot>,
    },
    /// Beginning the next turn, paused to ask about replacing that event (see CR 614.10).
    TurnStart(Resolving<L>),
    /// Combat that has banding assigns exact amounts before damage is dealt.
    ExactCombatDamage(DamageDivision<L>),
    /// Before the opening hands are drawn, `chooser` may reveal a card from outside the game
    /// as their companion (see CR 103.2b). The players with a card they may reveal are asked
    /// in turn order from the starting player.
    Companion { chooser: Seat },
    /// `player` pays for the companion special action (see CR 116.2g): `remaining` is the
    /// mana still to pay, by a payment plan or under manual payment one source at a time.
    CompanionPayment { player: Seat, remaining: ManaCost },
    /// The cards of merged or melded permanents that went to their owners' graveyards or
    /// libraries, which each owner may arrange (see [CR 731.3a]): "If a merged permanent is
    /// put into its owner's graveyard or library, that player may arrange the new objects in
    /// any order." `group` holds those still to arrange, two or more of each owner's in each
    /// zone; each pick puts the card picked lowest of those left in its owner's zone.
    ///
    /// [CR 731.3a]: https://yawgatog.com/resources/magic-rules/#R7313a
    ArrangePiles { group: Bitboard<L::Slot> },
    /// A triggered mana ability ([`beryl_card::Added::TypeProduced`]) triggered on tapping `slot`
    /// for mana that produced more than one type (`kinds`, bitmask of [`beryl_card::ManaColor`]
    /// indices), so `player` chooses one of those types `remaining` times (see CR 605.1b).
    ManaType {
        slot: L::Slot,
        player: Seat,
        kinds: u8,
        remaining: u8,
    },
    /// The attackers are declared and tapped, and the active player pays `remaining`, what is
    /// left of the total cost to attack (see [CR 508.1h], see [CR 508.1i]), one plan or one
    /// source at a time. The attack is announced once it is all paid: this is the last step
    /// of declaring attackers.
    ///
    /// [CR 508.1h]: https://yawgatog.com/resources/magic-rules/#R5081h
    /// [CR 508.1i]: https://yawgatog.com/resources/magic-rules/#R5081i
    PayingToAttack { remaining: ManaCost },
    /// The controller of the permanent in `slot` activates its mana-priced mana ability
    /// `ability` at priority and pays the remaining mana cost `remaining` of its price, by a
    /// payment plan or under manual payment one source at a time.
    ManaPrice {
        slot: L::Slot,
        ability: beryl_card::ObjectAbility,
        remaining: ManaCost,
    },
}

// The original derived hash wrote each variant's tag as an isize. The enum now has a
// byte-sized tag in memory, but the benchmark pins still read the original hash format.
impl<L: Layout> std::hash::Hash for Pending<L> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::None => 0_isize.hash(state),
            Self::PlayOrDraw { chooser } => (1_isize, chooser).hash(state),
            Self::Mulligans => 2_isize.hash(state),
            Self::Discard => 3_isize.hash(state),
            Self::LegendRule { kept } => (4_isize, kept).hash(state),
            Self::EnterPayingLife { slot, life } => (5_isize, slot, life).hash(state),
            Self::DeclareAttackers { attacker } => (6_isize, attacker).hash(state),
            Self::DeclareBlockers { blocker } => (7_isize, blocker).hash(state),
            Self::CombatDamage {
                source,
                done,
                lethal,
            } => {
                (8_isize, source, done, lethal).hash(state);
            }
            Self::Casting(casting) => (9_isize, casting).hash(state),
            Self::Resolving(resolving) => (10_isize, resolving).hash(state),
            Self::StepActions(resolving) => (11_isize, resolving).hash(state),
            Self::TurnStart(resolving) => (17_isize, resolving).hash(state),
            Self::TurningFaceUp {
                slot,
                cost,
                payment: FaceUpPayment::Mana(remaining),
            } if cost.parts().is_empty() => (13_isize, slot, remaining).hash(state),
            Self::TurningFaceUp {
                slot,
                cost,
                payment,
            } => {
                (13_isize, slot, cost, payment).hash(state);
            }
            Self::OrderEntered { group } => (14_isize, group).hash(state),
            Self::EnterChoosing {
                slot,
                kind,
                pay_life,
            } => {
                (15_isize, slot, kind, pay_life).hash(state);
            }
            Self::ManaColor { slot, ability } => (16_isize, slot, ability).hash(state),
            Self::ExactCombatDamage(dividing) => (18_isize, dividing).hash(state),
            Self::Companion { chooser } => (19_isize, chooser).hash(state),
            Self::CompanionPayment { player, remaining } => {
                (20_isize, player, remaining).hash(state);
            }
            Self::ArrangePiles { group } => (21_isize, group).hash(state),
            Self::ManaType {
                slot,
                player,
                kinds,
                remaining,
            } => (22_isize, slot, player, kinds, remaining).hash(state),
            Self::PayingToAttack { remaining } => (23_isize, remaining).hash(state),
            Self::ManaPrice {
                slot,
                ability,
                remaining,
            } => (24_isize, slot, ability, remaining).hash(state),
            Self::Outlet {
                slot,
                ability,
                paid,
            } => (25_isize, slot, ability, paid).hash(state),
        }
    }
}

impl Copy for Pending<Small> {}

impl<L: Layout> Pending<L> {
    /// The paused run the question belongs to: a resolution, a step's turn-based actions or
    /// the start of a turn, each run again from its start with the answers it records.
    pub fn paused(&self) -> Option<&Resolving<L>> {
        match self {
            Pending::Resolving(r) | Pending::StepActions(r) | Pending::TurnStart(r) => Some(r),
            Pending::None
            | Pending::PlayOrDraw { .. }
            | Pending::Mulligans
            | Pending::Discard
            | Pending::LegendRule { .. }
            | Pending::EnterPayingLife { .. }
            | Pending::DeclareAttackers { .. }
            | Pending::DeclareBlockers { .. }
            | Pending::CombatDamage { .. }
            | Pending::ExactCombatDamage(_)
            | Pending::Casting(_)
            | Pending::TurningFaceUp { .. }
            | Pending::OrderEntered { .. }
            | Pending::EnterChoosing { .. }
            | Pending::ManaColor { .. }
            | Pending::Outlet { .. }
            | Pending::ManaType { .. }
            | Pending::ManaPrice { .. }
            | Pending::Companion { .. }
            | Pending::CompanionPayment { .. }
            | Pending::PayingToAttack { .. }
            | Pending::ArrangePiles { .. } => None,
        }
    }
}

impl<L: Layout> State<L> {
    /// Opens the question of a machine step, or replaces the one open with its next: a
    /// casting that reached its next step is asked again whole. A resolution pauses through
    /// [`State::pause_resolution`], which keeps what it must remember.
    #[inline]
    pub fn ask(&mut self, question: Pending<L>) {
        debug_assert!(
            !matches!(
                question,
                Pending::None
                    | Pending::Resolving(_)
                    | Pending::StepActions(_)
                    | Pending::TurnStart(_)
            ),
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
    ///
    /// `asks_hidden` says the question is a hidden pick ([`Resolving`], "Hidden picks"). The
    /// record's hidden picks are kept from the pause before, together with the one its
    /// answer added.
    #[inline]
    pub fn pause_resolution(
        &mut self,
        record: L::Queue<L::Index, 32>,
        question: Decision<L>,
        asks_hidden: bool,
    ) {
        let (knowledge, hidden) = if let Pending::Resolving(r) = &self.pending {
            (r.knowledge.clone(), r.hidden)
        } else {
            (self.knowledge.clone(), 0)
        };
        self.pending = Pending::Resolving(Resolving {
            record,
            question,
            knowledge,
            hidden,
            asks_hidden,
        });
    }

    /// Pauses the turn-based actions of the step at `question`, a replacement effect's
    /// choice, with the answers given so far ([`Pending::StepActions`]). What each player
    /// knew as the actions began is kept from its first pause, as
    /// [`State::pause_resolution`] keeps it.
    #[inline]
    pub fn pause_step_actions(
        &mut self,
        record: L::Queue<L::Index, 32>,
        question: Decision<L>,
        starting_turn: bool,
    ) {
        let knowledge = if let Pending::StepActions(r) | Pending::TurnStart(r) = &self.pending {
            r.knowledge.clone()
        } else {
            self.knowledge.clone()
        };
        let paused = Resolving {
            record,
            question,
            knowledge,
            hidden: 0,
            asks_hidden: false,
        };
        self.pending = if starting_turn {
            Pending::TurnStart(paused)
        } else {
            Pending::StepActions(paused)
        };
    }

    /// Settles the open question so that a resolution's body, or a step's turn-based
    /// actions, run from their start. If it was paused, each player knows again what they knew
    /// as it began, which [`State::pause_resolution`] kept: a run asks its questions with the
    /// candidates the first run asked them with (architecture §7.3). Returns the cards some player knew
    /// beyond that, which the earlier runs revealed and chance must not count as revealed
    /// again ([`crate::chance`]); none for a resolution that was not paused.
    #[inline]
    pub fn resume_resolution(&mut self) -> Bitboard<L::Slot> {
        let revealed = match self.pending.paused() {
            Some(r) => {
                let knowledge = r.knowledge.clone();
                self.restore_knowledge(knowledge)
            }
            None => Bitboard::EMPTY,
        };
        self.pending = Pending::None;
        revealed
    }

    /// Records the answer to a paused resolution's or step's question, for it to run again
    /// with: for a hidden pick, the [`hidden_pick_answer`] of the slot picked, which the
    /// record then marks as one.
    ///
    /// # Panics
    ///
    /// If neither is paused.
    #[inline]
    pub fn record_answer(&mut self, answer: L::Index) -> Result<(), Full> {
        let (Pending::Resolving(r) | Pending::StepActions(r) | Pending::TurnStart(r)) =
            &mut self.pending
        else {
            unreachable!("a paused resolution or step");
        };
        if r.asks_hidden {
            // `hidden` marks the first 32 entries, one bit each, as many as a `Small`
            // record holds. A `Large` record may hold more, but a hidden pick past them
            // cannot be marked, so it is refused as a full record is.
            let position = u32::try_from(r.record.len()).map_err(|_| Full)?;
            r.hidden |= 1u32.checked_shl(position).ok_or(Full)?;
        }
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
    use crate::ids::Slot;
    use crate::profile::Profile;

    /// A paused resolution keeps what each player knew at its first pause through every
    /// later one, and resuming it gives exactly that back: a run of the body never starts
    /// knowing what an earlier run showed after a question.
    #[test]
    fn resuming_gives_back_what_was_known_at_the_first_pause() {
        let [known, first, second] = [0, 1, 2].map(Slot::from_index);
        let question = Decision {
            player: Seat::P0,
            decider: Seat::P0,
            kind: DecisionKind::Priority,
            candidates: Candidates::Objects(Bitboard::EMPTY),
            prompt: Prompt::new(Purpose::ALL[0]),
            about: Bitboard::EMPTY,
            filter: None,
        };
        let mut s = State::<Small>::empty(Profile::EXACT);
        s.board().set_known(Seat::P0, known, true);
        let began = s.knowledge();

        // Two runs pause, each after a card was shown that the one before had not shown.
        s.pause_resolution(Fixed::default(), question, false);
        s.look(&mut Sink::default(), Seat::P0, Bitboard::single(first));
        s.pause_resolution(Fixed::default(), question, false);
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
