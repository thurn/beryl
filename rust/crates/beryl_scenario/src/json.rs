//! The JSON dump of scenarios, for the oracle exporters (architecture §17.2).
//!
//! The scenario value types are `beryl_scenario_types`', which a card crate depends on and
//! which therefore knows no serializer (`docs/project-50k.md`, Crates). So this module
//! serializes them from outside: each has a mirror here, `…Def`, a serde remote definition
//! with the type's fields and the dump's attributes. The compiler holds a mirror to its type:
//! it reads every field it lists by name, and matches an enum's variants exhaustively.

use crate::{Live, Position, trace};
use beryl_engine::card::{Castings, FaceIx, ManaColor, ObjectAbility, Purpose};
use beryl_engine::state::CounterKind;
use beryl_engine::{CardTable, DecisionKind, Seat, TurnStep, Zone};
use beryl_scenario_types::{
    Action, Assertion, Counters, Ending, Permanent, Pick, PlayerSetup, Scenario, Setup,
};
use serde::{Serialize, Serializer};

/// A value type this module serializes through its mirror. Sealed: the mirrors are here.
pub trait Mirrored {
    #[doc(hidden)]
    fn mirror<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error>;
}

/// A scenario value as JSON, as the dump writes it: a [`Scenario`] as its name, setup and
/// actions, or one [`Action`].
pub struct Json<'a, T>(pub &'a T);

impl<T: Mirrored> Serialize for Json<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.mirror(s)
    }
}

macro_rules! mirrored {
    ($($type:ty => $mirror:ident),* $(,)?) => {$(
        impl Mirrored for $type {
            fn mirror<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                $mirror::serialize(self, s)
            }
        }
    )*};
}

mirrored! {
    Action => ActionDef,
    Counters => CountersDef,
    Permanent => PermanentDef,
    PlayerSetup => PlayerSetupDef,
    Scenario => ScenarioDef,
}

/// A list of mirrored values.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
fn list<T: Mirrored, S: Serializer>(values: &&'static [T], s: S) -> Result<S::Ok, S::Error> {
    s.collect_seq(values.iter().map(Json))
}

/// The two players' setups, as an array serializes.
fn players<S: Serializer>(players: &[PlayerSetup; 2], s: S) -> Result<S::Ok, S::Error> {
    [Json(&players[0]), Json(&players[1])].serialize(s)
}

#[allow(clippy::ref_option)] // the signature `serialize_with` calls
fn optional<T: Mirrored, S: Serializer>(value: &Option<T>, s: S) -> Result<S::Ok, S::Error> {
    value.as_ref().map(Json).serialize(s)
}

#[derive(Serialize)]
#[serde(remote = "Scenario")]
struct ScenarioDef {
    name: &'static str,
    #[serde(with = "SetupDef")]
    setup: Setup,
    #[serde(serialize_with = "list")]
    actions: &'static [Action],
    /// The oracles don't read it.
    #[serde(skip)]
    #[allow(dead_code)]
    interaction: &'static [&'static str],
}

#[derive(Serialize)]
#[serde(remote = "Setup")]
struct SetupDef {
    seed: u64,
    turn: u16,
    #[serde(serialize_with = "seat")]
    active: Seat,
    #[serde(serialize_with = "variant")]
    step: TurnStep,
    #[serde(serialize_with = "players")]
    players: [PlayerSetup; 2],
}

#[derive(Serialize)]
#[serde(remote = "PlayerSetup")]
struct PlayerSetupDef {
    life: i16,
    hand: &'static [&'static str],
    #[serde(serialize_with = "list")]
    battlefield: &'static [Permanent],
    graveyard: &'static [&'static str],
    exile: &'static [&'static str],
    library: &'static [&'static str],
}

#[allow(clippy::struct_excessive_bools)] // the mirror of a type with as many
#[derive(Serialize)]
#[serde(remote = "Permanent")]
struct PermanentDef {
    card: &'static str,
    tapped: bool,
    #[serde(serialize_with = "face_ix")]
    face: FaceIx,
    #[serde(serialize_with = "optional")]
    counters: Option<Counters>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    exiled_with: &'static [&'static str],
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    token: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    sick: bool,
}

#[derive(Serialize)]
#[serde(remote = "Counters")]
struct CountersDef {
    #[serde(serialize_with = "variant")]
    kind: CounterKind,
    count: u8,
}

#[derive(Serialize)]
#[serde(remote = "Action", tag = "action")]
pub(crate) enum ActionDef {
    PlayLand {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
    },
    Cast {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        #[serde(serialize_with = "face_ix")]
        face: FaceIx,
        #[serde(serialize_with = "castings")]
        costs: Castings,
    },
    Activate {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        #[serde(serialize_with = "ability_ix")]
        ability: ObjectAbility,
    },
    TurnFaceUp {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
    },
    Resolve,
    Answer {
        #[serde(serialize_with = "seat")]
        player: Seat,
        #[serde(serialize_with = "variant")]
        kind: DecisionKind,
        #[serde(with = "PickDef")]
        pick: Pick,
    },
    AdvanceTo {
        turn: u16,
        #[serde(serialize_with = "variant")]
        step: TurnStep,
    },
    Assert {
        #[serde(with = "AssertionDef")]
        assertion: Assertion,
    },
}

#[derive(Serialize)]
#[serde(remote = "Pick", tag = "pick")]
enum PickDef {
    Pass,
    Card {
        card: &'static str,
    },
    Number {
        n: u8,
    },
    Yes,
    No,
    Done,
    Player {
        #[serde(serialize_with = "seat")]
        player: Seat,
    },
    DefendingPlayer,
    Plan {
        i: u16,
    },
}

#[derive(Serialize)]
#[serde(remote = "Assertion", tag = "assert")]
enum AssertionDef {
    Life {
        #[serde(serialize_with = "seat")]
        player: Seat,
        life: i16,
    },
    Count {
        #[serde(serialize_with = "seat")]
        player: Seat,
        #[serde(serialize_with = "variant")]
        zone: Zone,
        card: Option<&'static str>,
        count: u8,
    },
    Tapped {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    Decision {
        #[serde(serialize_with = "seat")]
        player: Seat,
        #[serde(serialize_with = "variant")]
        kind: DecisionKind,
        candidates: u8,
    },
    Outcome {
        #[serde(with = "EndingDef")]
        outcome: Ending,
    },
    Controls {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    FaceDown {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    Attacking {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        count: u8,
    },
    Pt {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        token: bool,
        power: i8,
        toughness: i8,
    },
    Creature {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        creature: bool,
    },
    Counters {
        #[serde(serialize_with = "seat")]
        player: Seat,
        card: &'static str,
        #[serde(serialize_with = "variant")]
        kind: CounterKind,
        count: u8,
    },
    Stack {
        count: u8,
    },
    Library {
        #[serde(serialize_with = "seat")]
        player: Seat,
        top: &'static [&'static str],
    },
    Mana {
        #[serde(serialize_with = "seat")]
        player: Seat,
        #[serde(serialize_with = "variant")]
        color: ManaColor,
        amount: u8,
    },
}

#[derive(Serialize)]
#[serde(remote = "Ending", tag = "ending")]
enum EndingDef {
    InProgress,
    Win {
        #[serde(serialize_with = "seat")]
        player: Seat,
    },
    Draw,
}

/// Serializes an engine enum as its variant name, so the scenario types track the engine's
/// without mirroring them.
pub(crate) fn variant<T: std::fmt::Debug, S: Serializer>(
    value: &T,
    s: S,
) -> Result<S::Ok, S::Error> {
    s.collect_str(&format_args!("{value:?}"))
}

/// An ability of a permanent, as `{"Own": i}` or `{"Granted": k}`.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
pub(crate) fn ability_ix<S: Serializer>(ability: &ObjectAbility, s: S) -> Result<S::Ok, S::Error> {
    match *ability {
        ObjectAbility::Own(i) => s.serialize_newtype_variant("ObjectAbility", 0, "Own", &i.get()),
        ObjectAbility::Granted(k) => s.serialize_newtype_variant("ObjectAbility", 1, "Granted", &k),
    }
}

/// A face, as its index among the card's faces.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
pub(crate) fn face_ix<S: Serializer>(face: &FaceIx, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u8(face.get())
}

/// Casting options, as a bitmask over the face's list.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
pub(crate) fn castings<S: Serializer>(costs: &Castings, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u8(costs.bits())
}

/// A player, as their seat number.
#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
pub(crate) fn seat<S: Serializer>(player: &Seat, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u8(player.get())
}

#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)] // as `serialize_with` calls
pub(crate) fn optional_seat<S: Serializer>(player: &Option<Seat>, s: S) -> Result<S::Ok, S::Error> {
    match player {
        Some(p) => s.serialize_some(&p.get()),
        None => s.serialize_none(),
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // the signature `serialize_with` calls
pub(crate) fn purpose<S: Serializer>(purpose: &Purpose, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(purpose.name())
}

#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)] // as `serialize_with` calls
pub(crate) fn optional_purpose<S: Serializer>(
    purpose: &Option<Purpose>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match purpose {
        Some(p) => s.serialize_some(p.name()),
        None => s.serialize_none(),
    }
}

#[allow(clippy::ref_option)] // the signature `serialize_with` calls
pub(crate) fn optional_variant<T: std::fmt::Debug, S: Serializer>(
    value: &Option<T>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(v) => s.serialize_some(&format!("{v:?}")),
        None => s.serialize_none(),
    }
}

/// One scenario as the dump writes it: the card it belongs to, the scenario, and the position
/// each action began from, or why Beryl failed it.
#[derive(Serialize)]
struct Entry<'a> {
    card: &'a str,
    #[serde(flatten, with = "ScenarioDef")]
    scenario: Scenario,
    /// The index of the first of the actions the dump added: the snapshot's assertions.
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace: Option<Vec<Position>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<String>,
}

/// The scenarios of every card, each played on `table`, as a JSON array of `{card, name,
/// setup, actions, snapshot, trace}` objects (`failure` in place of `trace` for a scenario
/// Beryl fails): what `beryl_scenario dump` writes for the oracle exporters (§17.2). Each
/// scenario's actions end with the assertions of its final [`Live::snapshot`], so that an
/// oracle compares the whole public position it ends in, not only what its author asserted.
pub fn to_json(table: &'static CardTable, cards: &[(&str, &[Scenario])]) -> String {
    let entries: Vec<Entry> = cards
        .iter()
        .flat_map(|&(card, scenarios)| {
            scenarios.iter().map(move |scenario| {
                let (scenario, snapshot) = with_snapshot(table, scenario);
                let (trace, failure) = match trace(table, &scenario) {
                    Ok(t) => (Some(t), None),
                    Err(e) => (None, Some(e.to_string())),
                };
                Entry {
                    card,
                    scenario,
                    snapshot,
                    trace,
                    failure,
                }
            })
        })
        .collect();
    serde_json::to_string_pretty(&entries).expect("scenarios always serialize")
}

/// `scenario` with the assertions of its final snapshot after its own actions, and the index
/// of the first; unchanged where Beryl fails it, which [`trace`] then reports. The longer list
/// of actions is leaked: the dump is written once.
fn with_snapshot(table: &'static CardTable, scenario: &Scenario) -> (Scenario, Option<usize>) {
    let snapshot = || -> Result<Vec<Assertion>, String> {
        let mut live = Live::new(table, scenario).map_err(|f| f.message)?;
        // Assertions only check: the positions the others begin from are the same without.
        for &action in scenario.actions {
            if !matches!(action, Action::Assert { .. }) {
                live.act(action)?;
            }
        }
        live.snapshot()
    };
    match snapshot() {
        Ok(facts) if !facts.is_empty() => {
            let actions: Vec<Action> = scenario
                .actions
                .iter()
                .copied()
                .chain(facts.into_iter().map(Action::assert))
                .collect();
            let extended = Scenario {
                actions: Vec::leak(actions),
                ..*scenario
            };
            (extended, Some(scenario.actions.len()))
        }
        _ => (*scenario, None),
    }
}
