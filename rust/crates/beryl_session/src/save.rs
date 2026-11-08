//! What a choice in a save is (architecture §14): each answer recorded by what it does in the
//! rules (cast this card, attack with that creature, target that player) rather than by its
//! place among the decision's candidates, so a save outlives a reordered candidate list, a card
//! fix that leaves the chosen action legal, and prompts added or removed. Replay finds the
//! candidate that does the same ([`Chosen::find`]); the index it had is kept as a hint, which
//! only breaks ties between candidates that do the same and shows drift.

use beryl_engine::card::{AbilityIx, AbilityKind, CastingOption, ManaColor};
use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Layout, Opt, State, TurnStep, Zone};
use serde::{Deserialize, Serialize};

/// One answer in the choice log: what it does, and where it stood among the candidates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ChosenFile", into = "ChosenFile")]
pub struct Chosen {
    /// The answer's place among the candidates when it was made.
    pub index: u16,
    /// What it does; none in a version 1 save, which recorded the index alone.
    pub what: Option<Described>,
}

/// What an answer does: who answered what kind of decision, when, and the candidate they
/// took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Described {
    pub player: u8,
    pub turn: u16,
    pub step: StepName,
    pub kind: Kind,
    pub act: Act,
}

/// A [`Chosen`] as a file holds it: a bare index (version 1) or the whole description.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ChosenFile {
    Index(u16),
    Described {
        index: u16,
        player: u8,
        turn: u16,
        step: StepName,
        kind: Kind,
        act: Act,
    },
}

impl From<ChosenFile> for Chosen {
    fn from(file: ChosenFile) -> Chosen {
        match file {
            ChosenFile::Index(index) => Chosen { index, what: None },
            ChosenFile::Described {
                index,
                player,
                turn,
                step,
                kind,
                act,
            } => Chosen {
                index,
                what: Some(Described {
                    player,
                    turn,
                    step,
                    kind,
                    act,
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
                turn,
                step,
                kind,
                act,
            }) => ChosenFile::Described {
                index: chosen.index,
                player,
                turn,
                step,
                kind,
                act,
            },
        }
    }
}

impl Chosen {
    /// Candidate `c` of `d`, described in the position `s` it is asked in.
    pub fn new<L: Layout>(t: &GameTable, s: &State<L>, d: &Decision<L>, c: Choice) -> Chosen {
        Chosen {
            index: c.0,
            what: Some(Described {
                player: d.player.get(),
                turn: s.turn().number,
                step: s.turn().step.into(),
                kind: d.kind.into(),
                act: Act::of(t, s, d, c),
            }),
        }
    }

    /// The candidate of `d` that does what this answer did, if any: among several that do,
    /// the one at its old index, if that is one of them, else the first. A version 1 answer
    /// takes its index, if `d` has it.
    pub fn find<L: Layout>(&self, t: &GameTable, s: &State<L>, d: &Decision<L>) -> Option<Choice> {
        let n = d.len() as u16;
        let Some(what) = &self.what else {
            return (self.index < n).then_some(Choice(self.index));
        };
        if what.player != d.player.get()
            || Some(when(s)) != self.when()
            || what.kind != Kind::from(d.kind)
        {
            return None;
        }
        let same = |c: Choice| Act::of(t, s, d, c) == what.act;
        let hint = Choice(self.index);
        if !REVERSED.get() && self.index < n && same(hint) {
            return Some(hint);
        }
        candidates(n).find(|&c| same(c))
    }

    /// The turn and step the answer was made in, if the save says.
    pub fn when(&self) -> Option<(u16, StepName)> {
        self.what.as_ref().map(|w| (w.turn, w.step))
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

/// The turn and step the position `s` is in.
pub fn when<L: Layout>(s: &State<L>) -> (u16, StepName) {
    let turn = s.turn();
    (turn.number, turn.step.into())
}

/// The candidates of a decision of `n`, in order: reversed while a test asks
/// ([`reverse_candidates`]), as an engine that listed them the other way would.
fn candidates(n: u16) -> Box<dyn Iterator<Item = Choice>> {
    if REVERSED.get() {
        Box::new((0..n).rev().map(Choice))
    } else {
        Box::new((0..n).map(Choice))
    }
}

thread_local! {
    static REVERSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Has this thread's replays search each decision's candidates last first and ignore the old
/// index, while `on`: a save that replays the same either way depends on what its answers do,
/// not on where they stood. For tests.
#[doc(hidden)]
pub fn reverse_candidates(on: bool) {
    REVERSED.set(on);
}

/// What is being decided: [`DecisionKind`], under names a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Priority,
    PlayOrDraw,
    KeepOrMulligan,
    PutOnBottom,
    Discard,
    LegendRule,
    TriggerOrder,
    PayLife,
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
    Crew,
    ChooseUpTo,
    Waterbend,
    ExileCost,
}

impl From<DecisionKind> for Kind {
    fn from(kind: DecisionKind) -> Kind {
        // Exhaustive, so a new kind of decision can't go unnamed.
        match kind {
            DecisionKind::Priority => Kind::Priority,
            DecisionKind::PlayOrDraw => Kind::PlayOrDraw,
            DecisionKind::KeepOrMulligan => Kind::KeepOrMulligan,
            DecisionKind::PutOnBottom => Kind::PutOnBottom,
            DecisionKind::Discard => Kind::Discard,
            DecisionKind::LegendRule => Kind::LegendRule,
            DecisionKind::TriggerOrder => Kind::TriggerOrder,
            DecisionKind::PayLife => Kind::PayLife,
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
            DecisionKind::Crew => Kind::Crew,
            DecisionKind::ChooseUpTo => Kind::ChooseUpTo,
            DecisionKind::Waterbend => Kind::Waterbend,
            DecisionKind::ExileCost => Kind::ExileCost,
        }
    }
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
    /// A waiting triggered ability, by its source.
    Trigger {
        source: Option<Obj>,
    },
    Yes,
    No,
    Done,
    Attack {
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
    /// A payment plan, by its place among the plans: what tells plans apart is their
    /// consequences, which only the engine computes.
    Plan {
        i: u16,
    },
    Player {
        seat: u8,
    },
    /// An object, as an object candidate or [`Opt::Object`].
    Object {
        object: Obj,
    },
    TurnFaceUp {
        card: Obj,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)] // Serde's `skip_serializing_if` passes a reference.
fn is_false(b: &bool) -> bool {
    !b
}

impl Act {
    /// What candidate `c` of `d` does, in the position `s`.
    pub fn of<L: Layout>(t: &GameTable, s: &State<L>, d: &Decision<L>, c: Choice) -> Act {
        let obj = |slot| Obj::of(t, s, slot);
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
            Opt::Trigger(_) => Act::Trigger {
                source: o.object_named(s).map(obj),
            },
            Opt::Yes => Act::Yes,
            Opt::No => Act::No,
            Opt::Done => Act::Done,
            Opt::Attack(slot) => Act::Attack {
                creature: obj(slot),
            },
            Opt::Defender(slot) => Act::Defender {
                planeswalker: slot.map(obj),
            },
            Opt::Block(slot) => Act::Block {
                creature: obj(slot),
            },
            Opt::Lethal(slot) => Act::Lethal { blocker: obj(slot) },
            Opt::Cast { slot, face, costs } => {
                let options = t.def(s.card(slot)).face(face).casting;
                Act::Cast {
                    card: obj(slot),
                    back: face == beryl_engine::card::FaceIx::BACK,
                    face_down: face == beryl_engine::card::FaceIx::DOWN,
                    costs: options
                        .iter()
                        .enumerate()
                        .filter(|&(i, _)| costs.bits() & (1 << i) != 0)
                        .map(|(_, &option)| option.into())
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
            Opt::Number(n) => Act::Number { n },
            Opt::Plan(i) => Act::Plan {
                i: u16::try_from(Into::<usize>::into(i)).expect("a plan's index fits"),
            },
            Opt::Player(seat) => Act::Player { seat: seat.get() },
            Opt::Object(slot) => Act::Object { object: obj(slot) },
            Opt::TurnFaceUp(slot) => Act::TurnFaceUp { card: obj(slot) },
        }
    }
}

/// An object, by what the rules see of it: its name, zone and controller, and whether it is
/// tapped. Two objects alike in all of these are alike to the rules, but for their histories
/// (counters, damage), which a later version tells apart with names given as cards are
/// revealed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Obj {
    pub name: String,
    pub zone: ZoneName,
    pub controller: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    pub tapped: bool,
}

impl Obj {
    pub fn of<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot) -> Obj {
        let o = s.object(slot);
        Obj {
            name: s.name(t, slot).to_owned(),
            zone: o.zone.into(),
            controller: s.controller(slot).get(),
            tapped: o.flags.0 & beryl_engine::state::ObjectFlags::TAPPED != 0,
        }
    }
}

/// A [`Zone`], under names a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoneName {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Stack,
    Nowhere,
}

impl From<Zone> for ZoneName {
    fn from(zone: Zone) -> ZoneName {
        match zone {
            Zone::Library => ZoneName::Library,
            Zone::Hand => ZoneName::Hand,
            Zone::Battlefield => ZoneName::Battlefield,
            Zone::Graveyard => ZoneName::Graveyard,
            Zone::Exile => ZoneName::Exile,
            Zone::Stack => ZoneName::Stack,
            Zone::Nowhere => ZoneName::Nowhere,
        }
    }
}

/// A permanent's ability to activate: one of its face up's own, by its place among that
/// face's activated and mana abilities (so a static or triggered ability added to the card
/// moves nothing), or the `n`th ability other objects grant it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityRef {
    #[serde(default, skip_serializing_if = "is_false")]
    pub granted: bool,
    pub n: u8,
}

impl AbilityRef {
    fn of<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot, ability: AbilityIx) -> Self {
        match ability {
            AbilityIx::Own(i) => {
                let face = t.def(s.card(slot)).face(s.object(slot).face);
                let before = face.abilities[..usize::from(i)]
                    .iter()
                    .filter(|a| matches!(a.kind, AbilityKind::Activated | AbilityKind::Mana))
                    .count();
                AbilityRef {
                    granted: false,
                    n: u8::try_from(before).expect("a face's abilities number under 256"),
                }
            }
            AbilityIx::Granted(n) => AbilityRef { granted: true, n },
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
}

impl From<CastingOption> for CastingName {
    fn from(option: CastingOption) -> CastingName {
        match option {
            CastingOption::Kicker { .. } => CastingName::Kicker,
            CastingOption::Offspring(_) => CastingName::Offspring,
            CastingOption::Impending { .. } => CastingName::Impending,
            CastingOption::Overload { .. } => CastingName::Overload,
            CastingOption::Evoke { .. } => CastingName::Evoke,
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
