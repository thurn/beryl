//! What a choice in a save is (architecture §14): each answer recorded by what it does in the
//! rules (cast this card, attack with that creature, target that player) rather than by its
//! place among the decision's candidates, so a save outlives a reordered candidate list, a card
//! fix that leaves the chosen action legal, and prompts added or removed. Replay finds the
//! candidate that does the same ([`Chosen::find`]); the index it had is kept as a hint, which
//! only breaks ties between candidates that do the same and shows drift.

use beryl_engine::card::{AbilityKind, CastingOption, ChosenKind, FaceAbility, ManaColor};
use beryl_engine::chosen::ChosenValue;
use beryl_engine::state::StackItem;
use beryl_engine::{Choice, Decision, GameTable, Layout, Opt, Seat, State, TurnStep, Zone};
use serde::{Deserialize, Serialize};

use crate::perturb;

mod ability;
mod kind;
mod obj;

pub use ability::AbilityRef;
use kind::KindFile;
pub use kind::{Kind, KindBeforeV10};
pub use obj::{Obj, ZoneName};

/// One answer in the choice log: what it does, and where it stood among the candidates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ChosenFile", into = "ChosenFile")]
pub struct Chosen {
    /// The answer's place among the candidates when it was made.
    pub index: u16,
    /// Whether Undo groups this follow-up with the preceding answer.
    pub continuation: bool,
    /// What it does; none in a version 1 save, which recorded the index alone.
    pub what: Option<Described>,
}

/// What an answer does: whose decision of what kind it was and who answered it, when, and the
/// candidate they took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Described {
    /// The player the decision belonged to.
    pub player: u8,
    /// The player who answered it, where that was another player controlling `player`
    /// ([CR 724.5](https://yawgatog.com/resources/magic-rules/#R7245)), since version 12; none
    /// where `player` answered, as every answer before version 12 did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decider: Option<u8>,
    /// The turn's number in its own game, which starts again at 1 in a game Karn Liberated
    /// restarted and in a subgame.
    pub turn: u16,
    /// Turns begun across every game in this playthrough when the answer was made
    /// ([`State::elapsed_turns`]), which orders answers across a restart or a subgame, since
    /// version 15; none in an older file, whose answers replay orders by `turn`.
    pub elapsed: Option<u16>,
    pub step: StepName,
    /// The position as played when this game can repeat or reorder steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u32>,
    pub kind: Kind,
    /// The kind a file before version 10 gave it, where that format had a kind of its own for
    /// the question ([`KindBeforeV10`]). Replay matches an answer by `kind`; this says which
    /// question an old answer was to where replaying an old file turns on it.
    #[serde(
        default,
        rename = "kindBeforeV10",
        skip_serializing_if = "Option::is_none"
    )]
    pub before_v10: Option<KindBeforeV10>,
    pub act: Act,
    /// Where the candidate taken stood among those that do alike, in the order of the
    /// battlefield or its graveyard, since version 6: two permanents alike to one answer can
    /// differ to the answers after it, as two triggers that target one of them or both.
    pub nth: Option<u8>,
}

/// A [`Chosen`] as a file holds it: a bare index (version 1) or the whole description.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
#[expect(
    clippy::large_enum_variant,
    reason = "a file's form of an answer, read and converted to a `Chosen` at once"
)]
enum ChosenFile {
    Index(u16),
    Described {
        index: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        continuation: bool,
        player: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decider: Option<u8>,
        turn: u16,
        /// Written only where it differs from `turn`, as in a checkpoint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elapsed: Option<u16>,
        /// Marks an answer of a file before version 15, which did not record `elapsed`.
        #[serde(default, rename = "beforeV15", skip_serializing_if = "is_false")]
        before_v15: bool,
        step: StepName,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ordinal: Option<u32>,
        kind: KindFile,
        #[serde(
            default,
            rename = "kindBeforeV10",
            skip_serializing_if = "Option::is_none"
        )]
        kind_before_v10: Option<KindBeforeV10>,
        act: Act,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nth: Option<u8>,
    },
}

impl From<ChosenFile> for Chosen {
    fn from(file: ChosenFile) -> Chosen {
        match file {
            ChosenFile::Index(index) => Chosen {
                index,
                continuation: false,
                what: None,
            },
            ChosenFile::Described {
                index,
                continuation,
                player,
                decider,
                turn,
                elapsed,
                before_v15,
                step,
                ordinal,
                kind,
                kind_before_v10,
                act,
                nth,
            } => Chosen {
                index,
                continuation,
                what: Some(Described {
                    player,
                    decider,
                    turn,
                    elapsed: (!before_v15).then(|| elapsed.unwrap_or(turn)),
                    step,
                    ordinal,
                    kind: kind.now(),
                    before_v10: kind_before_v10.or(kind.before_v10()),
                    act,
                    nth,
                }),
            },
        }
    }
}

impl From<Chosen> for ChosenFile {
    fn from(chosen: Chosen) -> ChosenFile {
        match chosen.what {
            None => ChosenFile::Index(chosen.index),
            Some(Described {
                player,
                decider,
                turn,
                elapsed,
                step,
                ordinal,
                kind,
                before_v10,
                act,
                nth,
            }) => ChosenFile::Described {
                index: chosen.index,
                continuation: chosen.continuation,
                player,
                decider,
                turn,
                elapsed: elapsed.filter(|&e| e != turn),
                before_v15: elapsed.is_none(),
                step,
                ordinal,
                kind: KindFile::Now(kind),
                kind_before_v10: before_v10,
                act,
                nth,
            },
        }
    }
}

impl Chosen {
    /// Candidate `c` of `d`, described in the position `s` it is asked in.
    pub fn new<L: Layout>(t: &GameTable, s: &State<L>, d: &Decision<L>, c: Choice) -> Chosen {
        let act = Act::of(t, s, d, c);
        let alike: Vec<Choice> = (0..d.len() as u16)
            .map(Choice)
            .filter(|&other| Act::of(t, s, d, other) == act)
            .collect();
        let nth = in_order(s, d, alike)
            .and_then(|alike| alike.iter().position(|&other| other == c))
            .map(|i| u8::try_from(i).expect("a decision has under 256 candidates"));
        Chosen {
            index: c.0,
            continuation: false,
            what: Some(Described {
                player: d.player.get(),
                decider: (d.decider != d.player).then(|| d.decider.get()),
                turn: s.turn().number(),
                elapsed: Some(s.elapsed_turns()),
                step: s.turn().step().into(),
                ordinal: saved_ordinal(s),
                kind: d.kind.into(),
                before_v10: None,
                act,
                nth,
            }),
        }
    }

    /// The candidate of `d` that does what this answer did, if any: among several that do,
    /// the one at its old index, if that is one of them, else the first. A version 1 answer
    /// takes its index, if `d` has it. Candidates are in the order this thread lists them
    /// ([`perturb::candidates`]).
    pub fn find<L: Layout>(&self, t: &GameTable, s: &State<L>, d: &Decision<L>) -> Option<Choice> {
        let listed = perturb::candidates(d.len() as u16);
        let Some(what) = &self.what else {
            return listed.get(usize::from(self.index)).copied();
        };
        if what.player != d.player.get()
            || what.decider.unwrap_or(what.player) != d.decider.get()
            || self.compare_when(s) != Some(std::cmp::Ordering::Equal)
            || what.kind != Kind::from(d.kind)
        {
            return None;
        }
        // Each candidate is compared as the recorded answer's version would have named it,
        // whatever else the file says of it (a version 1 or 2 file is recorded again on this
        // build).
        let named = what.act.clone();
        let as_recorded = |act: Act| act.named_like(&named);
        let recorded = as_recorded(what.act.clone());
        let same = |c: Choice| {
            as_recorded(Act::of(t, s, d, c)) == recorded
                || as_recorded(Act::of_named(t, s, d, c, true)) == recorded
        };
        if let Some(nth) = what.nth {
            let alike = listed.iter().copied().filter(|&c| same(c)).collect();
            if let Some(c) =
                in_order(s, d, alike).and_then(|alike| alike.get(usize::from(nth)).copied())
            {
                return Some(c);
            }
        }
        let hint = listed.get(usize::from(self.index)).copied();
        if let Some(hint) = hint.filter(|&c| same(c)) {
            return Some(hint);
        }
        listed.into_iter().find(|&c| same(c))
    }

    /// The turn and step name the answer was made in, if the save says.
    pub fn when(&self) -> Option<(u16, StepName)> {
        self.what.as_ref().map(|w| (w.turn, w.step))
    }

    /// Where the recorded answer falls relative to the current step. Turns are ordered by
    /// the turns begun across the playthrough, since a turn's number starts again in a
    /// restarted game or a subgame; an answer from before version 15, which lacks that
    /// count, by its turn's number. Historical saves without an ordinal identify the step by
    /// name.
    pub(crate) fn compare_when<L: Layout>(&self, s: &State<L>) -> Option<std::cmp::Ordering> {
        let what = self.what.as_ref()?;
        let turn = s.turn();
        let in_turn = match what.ordinal {
            Some(ordinal) => ordinal
                .cmp(&s.step_position().ordinal)
                .then_with(|| what.step.cmp(&turn.step().into())),
            None => what.step.cmp(&turn.step().into()),
        };
        // A new game's opening, before its turn 1, has begun no turn of its own, so it shares
        // its count with the last turn of the game before it, which it follows.
        let by_turn = match what.elapsed {
            Some(elapsed) => {
                (elapsed, what.turn == 0).cmp(&(s.elapsed_turns(), turn.number() == 0))
            }
            None => what.turn.cmp(&turn.number()),
        };
        Some(by_turn.then(in_turn))
    }

    /// Whether this answer passed priority: an answer that no later position can miss, which
    /// replay skips where no priority is open ([`crate::Game::from_replay`]).
    pub fn is_priority_pass(&self) -> bool {
        matches!(
            &self.what,
            Some(Described {
                kind: Kind::Priority,
                act: Act::Pass,
                ..
            })
        )
    }
}

/// `alike`, candidates of `d` that do alike, in the order of the battlefield, of their
/// graveyards or of the stack, which the rules keep
/// ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137) orders permanents by
/// timestamp; [CR 404.2](https://yawgatog.com/resources/magic-rules/#R4042) keeps a graveyard's
/// order; [CR 405.2](https://yawgatog.com/resources/magic-rules/#R4052): "Each time an object is
/// put on the stack, it's put on top of all objects already there"): if there are two or more,
/// each naming an object in one of those zones. Storm copies alike but for their targets are
/// told apart by their places on the stack.
fn in_order<L: Layout>(
    s: &State<L>,
    d: &Decision<L>,
    mut alike: Vec<Choice>,
) -> Option<Vec<Choice>> {
    if alike.len() < 2 {
        return None;
    }
    let place = |c: Choice| {
        let slot = d
            .candidates
            .object(c)
            .or_else(|| d.candidates.option(c)?.object_named(s))?;
        let o = s.object(slot);
        match o.zone() {
            Zone::Battlefield => Some((0, s.battlefield().iter().position(|&b| b == slot)?)),
            // A shared graveyard, whose cards have no owner, is saved as the first seat's.
            Zone::Graveyard => Some((
                1 + o.owner().unwrap_or(Seat::P0).index(),
                s.graveyard(o.owner().unwrap_or(Seat::P0))
                    .iter()
                    .position(|&g| g == slot)?,
            )),
            Zone::Stack => Some((
                3,
                s.stack()
                    .iter()
                    .position(|e| e.spell().is_some_and(|spell| spell.slot == slot))?,
            )),
            _ => None,
        }
    };
    let mut places = Vec::with_capacity(alike.len());
    for &c in &alike {
        places.push((place(c)?, c));
    }
    places.sort_unstable_by_key(|&(place, _)| place);
    alike = places.into_iter().map(|(_, c)| c).collect();
    Some(alike)
}

/// Legacy saves already distinguish the ordinary steps by name. Write a position
/// for planned turns and when cleanup repeats in an ordinary turn.
pub(crate) fn saved_ordinal<L: Layout>(s: &State<L>) -> Option<u32> {
    let position = s.step_position();
    (s.has_turn_plan() || position.ordinal > u32::try_from(TurnStep::ALL.len()).unwrap())
        .then_some(position.ordinal)
}

/// A step of the turn ([`TurnStep`]), under names a save keeps, in turn order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepName {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    FirstStrikeDamage,
    CombatDamage,
    EndOfCombat,
    PostcombatMain,
    End,
    Cleanup,
}

impl From<TurnStep> for StepName {
    fn from(step: TurnStep) -> StepName {
        match step {
            TurnStep::Untap => StepName::Untap,
            TurnStep::Upkeep => StepName::Upkeep,
            TurnStep::Draw => StepName::Draw,
            TurnStep::PrecombatMain => StepName::PrecombatMain,
            TurnStep::BeginningOfCombat => StepName::BeginningOfCombat,
            TurnStep::DeclareAttackers => StepName::DeclareAttackers,
            TurnStep::DeclareBlockers => StepName::DeclareBlockers,
            TurnStep::FirstStrikeDamage => StepName::FirstStrikeDamage,
            TurnStep::CombatDamage => StepName::CombatDamage,
            TurnStep::EndOfCombat => StepName::EndOfCombat,
            TurnStep::PostcombatMain => StepName::PostcombatMain,
            TurnStep::End => StepName::End,
            TurnStep::Cleanup => StepName::Cleanup,
        }
    }
}

/// A candidate by what it does: [`Opt`], with each object it names as an [`Obj`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "act")]
pub enum Act {
    Pass,
    PlayLand {
        card: Obj,
    },
    Play,
    Draw,
    Keep,
    Mulligan,
    FreeMulligan,
    /// A waiting triggered ability, by its source and, since version 6, its place among the
    /// triggered abilities of the source's face: one permanent's two abilities may trigger
    /// on one event, as an evoked creature's own and its sacrifice
    /// ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)), and their order
    /// matters ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b)).
    Trigger {
        source: Option<Obj>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ability: Option<u8>,
    },
    Yes,
    No,
    Done,
    Attack {
        creature: Obj,
    },
    AttackInBand {
        creature: Obj,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first: Option<Box<Obj>>,
    },
    /// Every creature interchangeable with `creature` attacks, in one pick.
    AttackAll {
        creature: Obj,
    },
    /// What an attacker attacks: a planeswalker, or for none the defending player.
    Defender {
        planeswalker: Option<Obj>,
    },
    Block {
        creature: Obj,
    },
    Lethal {
        blocker: Obj,
    },
    /// Cast `card` with its back face up, if `back`, paying the casting options `costs`.
    Cast {
        card: Obj,
        #[serde(default, skip_serializing_if = "is_false")]
        back: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        face_down: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        costs: Vec<CastingName>,
    },
    Activate {
        card: Obj,
        ability: AbilityRef,
    },
    Mana {
        source: Obj,
        color: Color,
    },
    Pool {
        color: Color,
    },
    Number {
        n: u8,
    },
    /// A value of a catalog, by its name: "Red", "Goblin", "Counterspell", or "Another
    /// creature type" or "Another card" for a value no card of the game has. A game numbers its
    /// creature types and cards by its decklists, so the number a decision offers changes with
    /// them and the name does not.
    Chosen {
        value: String,
    },
    /// A payment plan, by its place among the plans: what tells plans apart is their
    /// consequences, which only the engine computes.
    Plan {
        i: u16,
    },
    Player {
        seat: u8,
    },
    /// An activated or triggered ability on the stack, as a target: by its source, whether it
    /// is triggered, and its place among the abilities of that kind of its source's face, or
    /// for an ability other objects grant, its place among those. Two alike, as two triggers
    /// of one ability, are told apart by their place among like ones ([`Described::nth`]).
    Ability {
        source: Option<Obj>,
        #[serde(default, skip_serializing_if = "is_false")]
        triggered: bool,
        ability: AbilityRef,
    },
    /// An object, as an object candidate or [`Opt::Object`].
    Object {
        object: Obj,
    },
    TurnFaceUp {
        card: Obj,
    },
    /// Put the revealed companion into its owner's hand.
    Companion,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde's `skip_serializing_if` passes a reference.
fn is_false(b: &bool) -> bool {
    !b
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde's `skip_serializing_if` passes a reference.
fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl Act {
    /// The object the act names, if any.
    fn object(&self) -> Option<&Obj> {
        match self {
            Act::PlayLand { card }
            | Act::Attack { creature: card }
            | Act::AttackInBand { creature: card, .. }
            | Act::AttackAll { creature: card }
            | Act::Block { creature: card }
            | Act::Lethal { blocker: card }
            | Act::Cast { card, .. }
            | Act::Activate { card, .. }
            | Act::Mana { source: card, .. }
            | Act::Object { object: card }
            | Act::TurnFaceUp { card } => Some(card),
            Act::Trigger { source: card, .. }
            | Act::Defender { planeswalker: card }
            | Act::Ability { source: card, .. } => card.as_ref(),
            _ => None,
        }
    }

    fn object_mut(&mut self) -> Option<&mut Obj> {
        match self {
            Act::PlayLand { card }
            | Act::Attack { creature: card }
            | Act::AttackInBand { creature: card, .. }
            | Act::AttackAll { creature: card }
            | Act::Block { creature: card }
            | Act::Lethal { blocker: card }
            | Act::Cast { card, .. }
            | Act::Activate { card, .. }
            | Act::Mana { source: card, .. }
            | Act::Object { object: card }
            | Act::TurnFaceUp { card } => Some(card),
            Act::Trigger { source: card, .. }
            | Act::Defender { planeswalker: card }
            | Act::Ability { source: card, .. } => card.as_mut(),
            _ => None,
        }
    }

    /// This candidate as the version that recorded `named` would have described it, given
    /// that `named` describes it alike: without what that version did not record.
    fn named_like(mut self, named: &Act) -> Act {
        if let Some(o) = named.object() {
            let before = obj::Before::of(o);
            if let Some(o) = self.object_mut() {
                o.name_like(before);
            }
        }
        if let (Act::Trigger { ability, .. }, Act::Trigger { ability: None, .. }) =
            (&mut self, named)
        {
            *ability = None;
        }
        self
    }

    /// What candidate `c` of `d` does, in the position `s`.
    pub fn of<L: Layout>(t: &GameTable, s: &State<L>, d: &Decision<L>, c: Choice) -> Act {
        Self::of_named(t, s, d, c, false)
    }

    /// The ability of the waiting trigger at `index`, as [`Act::Trigger`] names it: an own
    /// ability by its place among the face's triggered ones; one a copy exception gives by 128
    /// more than its place among the face's grants.
    fn trigger_ability<L: Layout>(t: &GameTable, s: &State<L>, index: usize) -> Option<u8> {
        s.triggers().get(index).map(|tr| {
            let FaceAbility::Own(own) = tr.of.ability.get() else {
                return tr.of.ability.get().number();
            };
            let triggered = t.def(tr.of.card).face(tr.of.face).abilities[..own.index()]
                .iter()
                .filter(|a| matches!(a.kind(), AbilityKind::Triggered))
                .count();
            u8::try_from(triggered).expect("a face's abilities number under 256")
        })
    }

    /// Older saves named face-down cards by the card behind them.
    fn of_named<L: Layout>(
        t: &GameTable,
        s: &State<L>,
        d: &Decision<L>,
        c: Choice,
        legacy: bool,
    ) -> Act {
        let obj = |slot| Obj::of(t, s, slot, legacy);
        if let Some(slot) = d.candidates.object(c) {
            return Act::Object { object: obj(slot) };
        }
        let o = d
            .candidates
            .option(c)
            .expect("a choice within the candidates");
        // Exhaustive, so a new option can't go undescribed.
        match o {
            Opt::Pass => Act::Pass,
            Opt::PlayLand(slot) => Act::PlayLand { card: obj(slot) },
            Opt::Play => Act::Play,
            Opt::Draw => Act::Draw,
            Opt::Keep => Act::Keep,
            Opt::Mulligan => Act::Mulligan,
            Opt::FreeMulligan => Act::FreeMulligan,
            Opt::Trigger(i) => Act::Trigger {
                source: o.object_named(s).map(obj),
                ability: Self::trigger_ability(t, s, i.into()),
            },
            Opt::Yes => Act::Yes,
            Opt::No => Act::No,
            Opt::Done => Act::Done,
            Opt::Attack { slot, band: None } => Act::Attack {
                creature: obj(slot),
            },
            Opt::AttackAll(slot) => Act::AttackAll {
                creature: obj(slot),
            },
            Opt::Defender(slot) => Act::Defender {
                planeswalker: slot.map(obj),
            },
            Opt::Block(slot) => Act::Block {
                creature: obj(slot),
            },
            Opt::Attack {
                slot,
                band: Some(first),
            } => Act::AttackInBand {
                creature: obj(slot),
                first: (slot != first).then(|| Box::new(obj(first))),
            },
            Opt::Lethal(slot) => Act::Lethal { blocker: obj(slot) },
            Opt::Cast { slot, face, costs } => {
                let options = s.copiable(slot).turned(t, face).face(t).casting;
                Act::Cast {
                    card: obj(slot),
                    back: face == beryl_engine::card::FaceIx::BACK,
                    face_down: face == beryl_engine::card::FaceIx::DOWN,
                    costs: options
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| costs.bits() & (1 << i) != 0)
                        .map(|(_, &option)| option.into())
                        .chain(costs.escapes().then_some(CastingName::Escape))
                        .collect(),
                }
            }
            Opt::Activate { slot, ability } => Act::Activate {
                card: obj(slot),
                ability: AbilityRef::of(t, s, slot, ability),
            },
            Opt::Mana { slot, color } => Act::Mana {
                source: obj(slot),
                color: color.into(),
            },
            Opt::Pool(color) => Act::Pool {
                color: color.into(),
            },
            Opt::Number(n) => match ChosenKind::of_purpose(d.prompt.purpose)
                .and_then(|kind| ChosenValue::<L>::of_number(kind, n))
            {
                Some(value) => Act::Chosen {
                    value: value.name(t),
                },
                None => Act::Number { n },
            },
            Opt::Plan(i) => Act::Plan {
                i: u16::try_from(Into::<usize>::into(i)).expect("a plan's index fits"),
            },
            Opt::Player(seat) => Act::Player { seat: seat.get() },
            Opt::Ability(place) => {
                let entry = &s.stack()[Into::<usize>::into(place)];
                Act::Ability {
                    source: o.object_named(s).map(obj),
                    triggered: matches!(entry.item, StackItem::Triggered { .. }),
                    ability: AbilityRef::on_stack(t, entry),
                }
            }
            Opt::Object(slot) => Act::Object { object: obj(slot) },
            Opt::TurnFaceUp(slot) => Act::TurnFaceUp { card: obj(slot) },
            Opt::Companion => Act::Companion,
        }
    }
}

/// A casting option ([`CastingOption`]) by its keyword.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CastingName {
    Kicker,
    Offspring,
    Impending,
    Overload,
    Evoke,
    Alternative,
    /// The escape an effect grants a card in a graveyard ([`Castings::ESCAPE`]).
    ///
    /// [`Castings::ESCAPE`]: beryl_engine::card::Castings::ESCAPE
    Escape,
    Mutate,
    Morph,
    Fuse,
    Prototype,
}

impl From<CastingOption> for CastingName {
    fn from(option: CastingOption) -> CastingName {
        match option {
            CastingOption::Kicker { .. } => CastingName::Kicker,
            CastingOption::Offspring(_) => CastingName::Offspring,
            CastingOption::Impending { .. } => CastingName::Impending,
            CastingOption::Overload { .. } => CastingName::Overload,
            CastingOption::Evoke { .. } => CastingName::Evoke,
            CastingOption::Alternative { .. } => CastingName::Alternative,
            CastingOption::Escape { .. } => CastingName::Escape,
            CastingOption::Mutate { .. } => CastingName::Mutate,
            CastingOption::Morph(_) => CastingName::Morph,
            CastingOption::Fuse => CastingName::Fuse,
            CastingOption::Prototype { .. } => CastingName::Prototype,
        }
    }
}

/// A [`ManaColor`], under names a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Color {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl From<ManaColor> for Color {
    fn from(color: ManaColor) -> Color {
        match color {
            ManaColor::White => Color::White,
            ManaColor::Blue => Color::Blue,
            ManaColor::Black => Color::Black,
            ManaColor::Red => Color::Red,
            ManaColor::Green => Color::Green,
            ManaColor::Colorless => Color::Colorless,
        }
    }
}
