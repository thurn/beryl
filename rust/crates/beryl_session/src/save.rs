//! What a choice in a save is (architecture §14): each answer recorded by what it does in the
//! rules (cast this card, attack with that creature, target that player) rather than by its
//! place among the decision's candidates, so a save outlives a reordered candidate list, a card
//! fix that leaves the chosen action legal, and prompts added or removed. Replay finds the
//! candidate that does the same ([`Chosen::find`]); the index it had is kept as a hint, which
//! only breaks ties between candidates that do the same and shows drift.

use beryl_engine::card::{AbilityKind, CastingOption, ManaColor, ObjectAbility, Types};
use beryl_engine::state::ObjectFlags;
use beryl_engine::{Choice, Decision, DecisionKind, GameTable, Layout, Opt, State, TurnStep, Zone};
use serde::{Deserialize, Serialize};

use crate::checkpoint::Counter;
use crate::perturb;

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

/// What an answer does: who answered what kind of decision, when, and the candidate they
/// took.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Described {
    pub player: u8,
    pub turn: u16,
    pub step: StepName,
    pub kind: Kind,
    pub act: Act,
    /// Where the candidate taken stood among those that do alike, in the order of the
    /// battlefield or its graveyard, since version 6: two permanents alike to one answer can
    /// differ to the answers after it, as two triggers that target one of them or both.
    pub nth: Option<u8>,
}

/// A [`Chosen`] as a file holds it: a bare index (version 1) or the whole description.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum ChosenFile {
    Index(u16),
    Described {
        index: u16,
        #[serde(default, skip_serializing_if = "is_false")]
        continuation: bool,
        player: u8,
        turn: u16,
        step: StepName,
        kind: Kind,
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
                turn,
                step,
                kind,
                act,
                nth,
            } => Chosen {
                index,
                continuation,
                what: Some(Described {
                    player,
                    turn,
                    step,
                    kind,
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
                turn,
                step,
                kind,
                act,
                nth,
            }) => ChosenFile::Described {
                index: chosen.index,
                continuation: chosen.continuation,
                player,
                turn,
                step,
                kind,
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
                turn: s.turn().number,
                step: s.turn().step.into(),
                kind: d.kind.into(),
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
            || Some(when(s)) != self.when()
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
        let same = |c: Choice| as_recorded(Act::of(t, s, d, c)) == recorded;
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

/// `alike`, candidates of `d` that do alike, in the order of the battlefield or of their
/// graveyards, which the rules keep ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137)
/// orders permanents by timestamp; [CR 404.2](https://yawgatog.com/resources/magic-rules/#R4042)
/// keeps a graveyard's order): if there are two or more, each naming an object in one of
/// those zones.
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
        match o.zone {
            Zone::Battlefield => Some((0, s.battlefield().iter().position(|&b| b == slot)?)),
            Zone::Graveyard => Some((
                1 + o.owner.index(),
                s.graveyard(o.owner).iter().position(|&g| g == slot)?,
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

/// The turn and step the position `s` is in.
pub fn when<L: Layout>(s: &State<L>) -> (u16, StepName) {
    let turn = s.turn();
    (turn.number, turn.step.into())
}

/// What is being decided: [`DecisionKind`], under names a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

impl Kind {
    /// Every kind, in order. A new kind goes here as well as in the match below, which the
    /// compiler holds to every [`DecisionKind`].
    pub const ALL: [Kind; 25] = [
        Kind::Priority,
        Kind::PlayOrDraw,
        Kind::KeepOrMulligan,
        Kind::PutOnBottom,
        Kind::Discard,
        Kind::LegendRule,
        Kind::TriggerOrder,
        Kind::PayLife,
        Kind::Attacker,
        Kind::AttackTarget,
        Kind::Blocker,
        Kind::BlockTarget,
        Kind::DamageDivision,
        Kind::X,
        Kind::TargetCount,
        Kind::Target,
        Kind::PaymentPlan,
        Kind::ManaPayment,
        Kind::CastOffer,
        Kind::YesNo,
        Kind::ChooseObject,
        Kind::Crew,
        Kind::ChooseUpTo,
        Kind::Waterbend,
        Kind::ExileCost,
    ];
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
            | Act::Block { creature: card }
            | Act::Lethal { blocker: card }
            | Act::Cast { card, .. }
            | Act::Activate { card, .. }
            | Act::Mana { source: card, .. }
            | Act::Object { object: card }
            | Act::TurnFaceUp { card } => Some(card),
            Act::Trigger { source: card, .. } | Act::Defender { planeswalker: card } => {
                card.as_ref()
            }
            _ => None,
        }
    }

    fn object_mut(&mut self) -> Option<&mut Obj> {
        match self {
            Act::PlayLand { card }
            | Act::Attack { creature: card }
            | Act::Block { creature: card }
            | Act::Lethal { blocker: card }
            | Act::Cast { card, .. }
            | Act::Activate { card, .. }
            | Act::Mana { source: card, .. }
            | Act::Object { object: card }
            | Act::TurnFaceUp { card } => Some(card),
            Act::Trigger { source: card, .. } | Act::Defender { planeswalker: card } => {
                card.as_mut()
            }
            _ => None,
        }
    }

    /// This candidate as the version that recorded `named` would have described it, given
    /// that `named` describes it alike: without what that version did not record.
    fn named_like(mut self, named: &Act) -> Act {
        if let Some(o) = named.object() {
            let (before_v5, before_v6) = (o.before_v5, o.before_v6);
            if let Some(o) = self.object_mut() {
                o.name_like(before_v5, before_v6);
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
            Opt::Trigger(i) => Act::Trigger {
                source: o.object_named(s).map(obj),
                ability: s.triggers().get(Into::<usize>::into(i)).map(|tr| {
                    let triggered = t.def(tr.card).face(tr.face).abilities[..tr.ability.index()]
                        .iter()
                        .filter(|a| matches!(a.kind, AbilityKind::Triggered))
                        .count();
                    u8::try_from(triggered).expect("a face's abilities number under 256")
                }),
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
                let options = s.copiable(slot).turned(face).face(t).casting;
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

/// An object, by what the rules see of it: its name, zone, controller and owner, whether it
/// is tapped, and for a permanent, since version 6, whether it is a token, summoning sick,
/// its counters, the damage marked on it and its place in combat. Copies of a card alike in
/// all of these are alike to the rules for one answer; [`Described::nth`] tells them apart
/// for the answers after it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "an object's statuses, each yes or no in the rules"
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Obj {
    pub name: String,
    pub zone: ZoneName,
    pub controller: u8,
    /// Its owner, where that is not its controller, since version 5. Two copies of a card one
    /// player controls, one of them another player's, part ways where the rules send a card
    /// to its owner's zone ([CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083)):
    /// "The owner of a card in the game is the player who started the game with it in their
    /// deck."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<u8>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub tapped: bool,
    /// A card in its owner's hand or library that the other player knows, since version 5.
    /// Playing a copy they have seen tells them nothing; playing one they haven't leaves them
    /// still not knowing the other, in a hidden zone
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)).
    #[serde(default, skip_serializing_if = "is_false")]
    pub shown: bool,
    /// Named by a version 4 save or older, which names neither the owner nor whether the other
    /// player knows it: replay takes an object either way, as version 4 did.
    #[serde(default, rename = "beforeV5", skip_serializing_if = "is_false")]
    pub before_v5: bool,
    /// A token, since version 6: a token copy of a card is a permanent apart from the card
    /// ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)).
    #[serde(default, skip_serializing_if = "is_false")]
    pub token: bool,
    /// A creature that cannot yet attack or {T} ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)):
    /// "A creature's activated ability with the tap symbol or the untap symbol in its
    /// activation cost can't be activated unless the creature has been under its controller's
    /// control continuously since their most recent turn began." Since version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sick: bool,
    /// A permanent's counters, since version 6.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counters: Vec<Counter>,
    /// The damage marked on a permanent, since version 6.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub damage: u8,
    /// An attacking creature ([CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064)),
    /// since version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub attacking: bool,
    /// A blocking creature, since version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub blocking: bool,
    /// An attacking creature that was blocked, which stays blocked though its blockers leave
    /// combat ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h)), since
    /// version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub blocked: bool,
    /// The planeswalker an attacking creature attacks, or the creature a blocking one blocks,
    /// by name, since version 6: two attackers alike but for whom they attack differ to the
    /// defender ([CR 506.3](https://yawgatog.com/resources/magic-rules/#R5063)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub against: Option<String>,
    /// Named by a version 5 save or older, which names none of a permanent's token, summoning
    /// sickness, counters, damage and place in combat: replay takes an object whatever they
    /// are.
    #[serde(default, rename = "beforeV6", skip_serializing_if = "is_false")]
    pub before_v6: bool,
}

impl Obj {
    pub fn of<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot) -> Obj {
        let o = s.object(slot);
        let controller = s.controller(slot);
        let permanent = o.zone == Zone::Battlefield;
        Obj {
            name: s.name(t, slot).to_owned(),
            zone: o.zone.into(),
            controller: controller.get(),
            owner: (o.owner != controller).then_some(o.owner.get()),
            tapped: o.flags.has(ObjectFlags::TAPPED),
            shown: matches!(o.zone, Zone::Hand | Zone::Library)
                && s.knows(o.owner.opponent(), slot),
            before_v5: false,
            token: permanent && o.flags.has(ObjectFlags::TOKEN),
            sick: permanent
                && o.flags.has(ObjectFlags::SUMMONING_SICK)
                && s.derived(slot).types.contains(Types::CREATURE),
            counters: if permanent {
                Counter::on(o)
            } else {
                Vec::new()
            },
            damage: if permanent { o.damage } else { 0 },
            attacking: permanent && o.flags.has(ObjectFlags::ATTACKING),
            blocking: permanent && o.flags.has(ObjectFlags::BLOCKING),
            blocked: permanent && o.flags.has(ObjectFlags::BLOCKED),
            against: o
                .combat
                .filter(|&other| {
                    permanent
                        && o.flags.0 & (ObjectFlags::ATTACKING | ObjectFlags::BLOCKING) != 0
                        && s.object(other).zone == Zone::Battlefield
                })
                .map(|other| s.name(t, other).to_owned()),
            before_v6: false,
        }
    }

    /// This object as a version before 5, if `before_v5`, or before 6, if `before_v6`, named
    /// it.
    fn name_like(&mut self, before_v5: bool, before_v6: bool) {
        if before_v5 {
            self.owner = None;
            self.shown = false;
            self.before_v5 = true;
        }
        if before_v6 {
            self.token = false;
            self.sick = false;
            self.counters.clear();
            self.damage = 0;
            self.attacking = false;
            self.blocking = false;
            self.blocked = false;
            self.against = None;
            self.before_v6 = true;
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
    fn of<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot, ability: ObjectAbility) -> Self {
        match ability {
            ObjectAbility::Own(i) => {
                let face = s.copiable(slot).face(t);
                let before = face.abilities[..i.index()]
                    .iter()
                    .filter(|a| matches!(a.kind, AbilityKind::Activated | AbilityKind::Mana))
                    .count();
                AbilityRef {
                    granted: false,
                    n: u8::try_from(before).expect("a face's abilities number under 256"),
                }
            }
            ObjectAbility::Granted(n) => AbilityRef { granted: true, n },
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
