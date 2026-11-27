//! What the dump says of each card a scenario names: its faces' abilities, costs, casting
//! options and the facts of their effect data, read from the definition alone by a walk
//!. The oracle exporters translate a scenario's decisions
//! into each oracle's terms from these, so they
//! keep no table keyed by card name. Nothing here names a card or adds to the vocabulary.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

use beryl_card::definition::Shape as CardShape;
use beryl_card::{
    Ability, AbilityKind, Amount, BasicLandTypes, Behavior, CardDef, CastingOption, Cost,
    CostAction, CostPart, Effect, Face, FaceIx, ManaColor, ManaCost, ManaSet, Measure,
    Modification, Objects, Players, Taken, TargetSpec, Words,
};
use beryl_engine::CardTable;
use serde::Serialize;
use serde_json::Value;

use crate::{Action, Pick, Scenario};

/// One card: its faces, front first.
#[derive(Debug, Serialize)]
pub struct CardFacts {
    pub shape: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fuse: bool,
    pub faces: Vec<FaceFacts>,
}

/// One face: its name, its abilities in the order scenarios number them, those it grants to
/// other objects, and its casting options in the order of a `Cast`'s `costs` bits.
#[derive(Debug, Serialize)]
pub struct FaceFacts {
    pub name: &'static str,
    /// An alternate face that can't be cast normally. Kept under its original wire name.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub transformed: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub abilities: Vec<AbilityFacts>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<AbilityFacts>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub casting: Vec<CastingFacts>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additional_cost: Vec<PartFacts>,
}

/// One ability.
// A JSON record the oracle tools read: each flag is a field of its own there.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Serialize)]
pub struct AbilityFacts {
    /// As a scenario names it: `Own i` over the face's abilities, or `Grant i` over those it
    /// grants. An object's `Granted k` is the `k`th ability granted to it, counted in the
    /// order granted, so the granting face and its `Grant` index are the game's to say.
    pub index: String,
    /// Spell, Activated, Loyalty, Mana, Triggered, Static or Replacement.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostFacts>,
    /// A mana ability's mana.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produces: Option<ManaFacts>,
    /// A mana ability's "Activate only if you control ...".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conditional: bool,
    /// A mana ability's "Spend this mana only ...": its mana remembers a restriction (CR 106.6).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restricted: bool,
    /// A triggered ability's event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetFacts>,
    /// Its effect data's instructions, in the order written.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instructions: Vec<Instruction>,
    /// A Rust body, which nothing can read.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
    /// A static ability that sets or adds basic land types, as Blood Moon's or Urborg's:
    /// lands then tap for mana their own text doesn't write, so an exporter can't name the
    /// mana ability a payment taps from a land's text.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub changes_land_types: bool,
    /// A replacement effect that may have its permanent enter as a copy of a permanent its
    /// controller chooses (`Rewrite::EnterAsCopy`): Beryl asks one pick of the permanent, or
    /// none, where an oracle may ask whether to copy first.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub enters_as_copy: bool,
    /// A level up ability (CR 702.87a), which an oracle names by its keyword rather than by
    /// a cost and effect its Oracle text does not write out.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub level_up: bool,
}

/// A cost: its mana in Scryfall's notation, then its other parts in the order printed.
#[derive(Debug, Serialize)]
pub struct CostFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mana: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartFacts>,
}

/// One part of a cost that is not mana.
#[derive(Debug, Default, Serialize)]
pub struct PartFacts {
    /// Tap, Sacrifice, Discard, Exile, Reveal or `RemoveCounter`; Loyalty, Life or `OneOf`.
    pub action: String,
    /// Done to the object the cost is on (`This`) or to objects its payer chooses (`Chosen`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub of: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter: Option<String>,
    /// Where chosen objects are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub other: bool,
    /// How many chosen objects pay it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u8>,
    /// Chosen objects of at least this total power, as crew's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_power: Option<u8>,
    /// Chosen objects tapped each for one generic mana, up to this many, as waterbend's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_generic: Option<u8>,
    /// Loyalty counters put on (positive) or removed, or life paid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i16>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub one_of: Vec<CostFacts>,
}

/// The mana a mana ability adds: `amount` mana of one of `colors`, a choice when there are
/// several.
#[derive(Debug, Serialize)]
pub struct ManaFacts {
    pub colors: Vec<String>,
    pub amount: u8,
}

/// One casting option, at its bit of a `Cast`'s `costs`.
#[derive(Debug, Serialize)]
pub struct CastingFacts {
    pub bit: usize,
    /// The keyword: Kicker, Offspring, Impending, Overload or Evoke.
    pub keyword: String,
    /// Paid rather than the mana cost; else on top of it.
    pub alternative: bool,
    pub cost: CostFacts,
    /// The cost to turn a permanent with morph face up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_up: Option<CostFacts>,
    /// An overload's changed text: the face's ability the spell has instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ability: Option<String>,
}

/// One instance of the word "target".
#[derive(Debug, Serialize)]
pub struct TargetFacts {
    /// What it may choose: Permanent, Spell, `SpellOrPermanent`, `GraveyardCard` or Player.
    pub kind: String,
    pub count: u8,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub other: bool,
}

/// One instruction of effect data. "May", "if", "each player" and sequences are not
/// instructions of their own: they shape those inside them, which say `may` and name the
/// players for whom each is done.
#[derive(Debug, Default, Serialize)]
pub struct Instruction {
    /// The vocabulary's name for it: Scry, Search, Dig, Choose, Unless, Counter, ...
    pub kind: String,
    /// Inside a "[player] may".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub may: bool,
    /// Who does it or has it done to them: You, `EachOpponent`, `EachPlayer`, `ThatPlayer`,
    /// `Target i`, `ControllerOf ...`, `OwnerOf ...`; inside an "each player" its players.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player: Option<String>,
    /// A second set of players, as the player whose library is fatesealed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub others: Option<String>,
    /// What it is done to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objects: Option<String>,
    /// Its number: a fixed one, or the name of the expression it is read from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<Value>,
    /// Fixed damage divided among the targets announced during casting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divided: Option<u8>,
    /// A choice's "you may".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// A choice made without revealing it (CR 101.4a), which each player is asked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// A dig's cards taken into the hand: a number, or `MayReveal` for "you may reveal one".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take: Option<Value>,
    /// A dig's cards put on top of the library before the rest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<u8>,
    /// A dig's order for the rest, or where a search puts what it finds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest: Option<String>,
    /// What "unless [player] pays" asks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostFacts>,
}

/// The facts of each card `scenario` of `card` names: its own card, every card of its setup
/// and every card its actions name, with the tokens each can create, by name.
pub fn named(
    table: &'static CardTable,
    card: &str,
    scenario: &Scenario,
) -> BTreeMap<&'static str, CardFacts> {
    let known = known(table);
    let mut names: Vec<&'static str> = known.get(card).map(|def| def.name).into_iter().collect();
    for player in &scenario.setup.players {
        names.extend(player.hand);
        names.extend(player.graveyard);
        names.extend(player.exile);
        names.extend(player.library);
        names.extend(player.sideboard);
        for permanent in player.all_permanents() {
            names.push(permanent.card);
            names.extend(permanent.exiled_with);
        }
    }
    names.extend(scenario.actions.iter().filter_map(action_card));
    let mut out = BTreeMap::new();
    while let Some(name) = names.pop() {
        let Some(&def) = known.get(name) else {
            continue;
        };
        if out.contains_key(def.name) {
            continue;
        }
        names.extend(def.tokens.iter().map(|token| token.name));
        out.insert(def.name, card_facts(def));
    }
    out
}

/// The card an action names, if any.
fn action_card(action: &Action) -> Option<&'static str> {
    match *action {
        Action::PlayLand { card, .. }
        | Action::Cast { card, .. }
        | Action::Activate { card, .. }
        | Action::TurnFaceUp { card, .. }
        | Action::Answer {
            pick: Pick::Cast { card, .. },
            ..
        } => Some(card),
        _ => None,
    }
}

/// Every card of `table` and every token or emblem one can create, by name.
fn known(table: &'static CardTable) -> HashMap<&'static str, &'static CardDef> {
    let mut known = HashMap::new();
    let mut work: Vec<&'static CardDef> = table.cards().to_vec();
    while let Some(def) = work.pop() {
        if known.insert(def.name, def).is_none() {
            work.extend(def.tokens);
        }
    }
    known
}

/// The facts of one card.
pub fn card_facts(def: &'static CardDef) -> CardFacts {
    CardFacts {
        shape: variant(&def.shape),
        fuse: matches!(def.shape, CardShape::Split { fuse: true }),
        faces: def
            .face_ixs()
            .map(|ix| {
                face_facts(
                    def.face_name(ix),
                    def.face(ix),
                    ix == FaceIx::BACK
                        && !def.shape.can_cast_back()
                        && !def.face(ix).is_prototyped(),
                )
            })
            .collect(),
    }
}

fn face_facts(name: &'static str, face: &'static Face, transformed: bool) -> FaceFacts {
    let numbered = |prefix: &str, abilities: &'static [Ability]| {
        abilities
            .iter()
            .enumerate()
            .map(|(i, ability)| ability_facts(format!("{prefix} {i}"), ability))
            .collect()
    };
    FaceFacts {
        name,
        transformed,
        abilities: numbered("Own", face.abilities),
        grants: numbered("Grant", face.grants),
        casting: face
            .casting
            .iter()
            .enumerate()
            .map(|(bit, &option)| casting_facts(bit, option))
            .collect(),
        additional_cost: face.additional_cost.iter().map(part_facts).collect(),
    }
}

fn ability_facts(index: String, ability: &Ability) -> AbilityFacts {
    let kind = match ability.kind() {
        AbilityKind::Activated if ability.is_loyalty() => "Loyalty",
        AbilityKind::Spell => "Spell",
        AbilityKind::Activated => "Activated",
        AbilityKind::Mana => "Mana",
        AbilityKind::Triggered => "Triggered",
        AbilityKind::Static => "Static",
        AbilityKind::Replacement => "Replacement",
    };
    let mut instructions = Vec::new();
    let custom = match ability.body() {
        Behavior::Data(effect) => {
            walk(effect, &Shape::default(), &mut instructions);
            false
        }
        Behavior::Custom(_) => true,
    };
    AbilityFacts {
        index,
        kind,
        cost: cost_facts(ability.cost()),
        produces: (ability.kind() == AbilityKind::Mana).then(|| mana_facts(ability.produces())),
        conditional: ability.only_if_you_control().is_some(),
        restricted: ability.spend().is_some(),
        trigger: ability.trigger().map(|trigger| variant(&trigger.event)),
        targets: ability.targets().iter().map(target_facts).collect(),
        instructions,
        custom,
        changes_land_types: ability.layer().is_some_and(|spec| {
            spec.modifications.iter().any(|m| match *m {
                Modification::SetLandTypeWords(_) => true,
                // The card's text as printed: what scenarios are made from.
                Modification::AddSubtypeWord(word) => {
                    BasicLandTypes::of_subtype(word.read(&Words::NONE)).is_some()
                }
                _ => false,
            })
        }),
        enters_as_copy: ability
            .replacement()
            .is_some_and(|r| matches!(r.rewrite, beryl_card::Rewrite::EnterAsCopy { .. })),
        level_up: ability.is_level_up(),
    }
}

/// What the instructions around one say of it.
#[derive(Clone, Default)]
struct Shape {
    may: bool,
    /// Inside an "each player": who [`Players::ThatPlayer`] stands for.
    that_player: Option<String>,
}

/// Appends the instructions of `effect` to `out`, in the order written.
fn walk(effect: &Effect, shape: &Shape, out: &mut Vec<Instruction>) {
    match *effect {
        Effect::Sequence(effects) => {
            for effect in effects {
                walk(effect, shape, out);
            }
        }
        Effect::If {
            then, otherwise, ..
        } => {
            walk(then, shape, out);
            if let Some(otherwise) = otherwise {
                walk(otherwise, shape, out);
            }
        }
        Effect::May { effect, .. } => {
            let inner = Shape {
                may: true,
                ..shape.clone()
            };
            walk(effect, &inner, out);
        }
        Effect::ForEachPlayer { players, effect } => {
            let inner = Shape {
                that_player: Some(players_name(&players, shape)),
                ..shape.clone()
            };
            walk(effect, &inner, out);
        }
        Effect::Unless {
            effect,
            player,
            cost,
        } => {
            out.push(Instruction {
                cost: cost_facts(&cost),
                player: Some(players_name(&player, shape)),
                ..instruction("Unless", shape)
            });
            walk(effect, shape, out);
        }
        _ => out.push(leaf(effect, shape)),
    }
}

fn instruction(kind: &str, shape: &Shape) -> Instruction {
    Instruction {
        kind: kind.to_owned(),
        may: shape.may,
        ..Instruction::default()
    }
}

/// An instruction with nothing inside it: its operands, and what its kind says besides.
fn leaf(effect: &Effect, shape: &Shape) -> Instruction {
    let operands = effect.operands();
    let mut out = Instruction {
        player: operands.players.map(|p| players_name(p, shape)),
        others: operands.others.map(|p| players_name(p, shape)),
        objects: operands.objects.map(objects_name),
        count: operands.amount.map(amount_value),
        ..instruction(&variant(effect), shape)
    };
    match *effect {
        Effect::Choose {
            optional, hidden, ..
        } => {
            out.optional = optional;
            out.hidden = hidden;
        }
        Effect::Dig {
            take, top, rest, ..
        } => {
            out.take = Some(match take {
                Taken::None => Value::from(0),
                Taken::Put(n) => Value::from(n),
                Taken::MayReveal(_) => Value::from("MayReveal"),
            });
            out.top = (top > 0).then_some(top);
            out.rest = Some(match rest {
                beryl_card::LibraryRest::Bottom(order) => variant(&order),
                beryl_card::LibraryRest::Top => "Top".to_owned(),
            });
        }
        Effect::Search { found, .. } => out.rest = Some(variant(&found)),
        Effect::DealDamage {
            to: beryl_card::Recipients::DividedTwo(_),
            amount: Amount::Fixed(total),
        } => {
            out.divided = Some(total);
        }
        _ => {}
    }
    out
}

fn players_name(players: &Players, shape: &Shape) -> String {
    match *players {
        Players::ThatPlayer => shape
            .that_player
            .clone()
            .unwrap_or_else(|| "ThatPlayer".to_owned()),
        Players::Target(i) => format!("Target {}", i.get()),
        Players::ControllerOf(objects) => format!("ControllerOf {}", objects_name(&objects)),
        Players::OwnerOf(objects) => format!("OwnerOf {}", objects_name(&objects)),
        Players::Enchanted => "Enchanted".to_owned(),
        Players::SubgameNonwinners => "SubgameNonwinners".to_owned(),
        Players::EachOther(players) => format!("EachOther {}", players_name(players, shape)),
        Players::You | Players::EachOpponent | Players::EachPlayer => variant(players),
    }
}

fn objects_name(objects: &Objects) -> String {
    match *objects {
        Objects::Target(i) => format!("Target {}", i.get()),
        Objects::InZoneOf { zone, .. } => format!("InZoneOf {}", variant(&zone)),
        Objects::All { zone, .. } => format!("All {}", variant(&zone)),
        _ => variant(objects),
    }
}

fn amount_value(amount: &Amount) -> Value {
    match *amount {
        Amount::Fixed(n) => Value::from(n),
        _ => Value::from(variant(amount)),
    }
}

/// `None` for a cost of nothing.
fn cost_facts(cost: &Cost) -> Option<CostFacts> {
    let mana = mana_text(cost.mana);
    let parts: Vec<PartFacts> = cost.parts().iter().map(part_facts).collect();
    (mana.is_some() || !parts.is_empty()).then_some(CostFacts { mana, parts })
}

fn part_facts(part: &CostPart) -> PartFacts {
    let action = |done: CostAction| match done {
        CostAction::RemoveCounter(kind) => ("RemoveCounter".to_owned(), Some(variant(&kind))),
        _ => (variant(&done), None),
    };
    match *part {
        CostPart::This(done) => {
            let (action, counter) = action(done);
            PartFacts {
                action,
                of: Some("This"),
                counter,
                ..PartFacts::default()
            }
        }
        CostPart::Chosen(done, chosen) => {
            let (action, counter) = action(done);
            let mut out = PartFacts {
                action,
                of: Some("Chosen"),
                counter,
                zone: Some(variant(&chosen.zone)),
                other: chosen.other,
                ..PartFacts::default()
            };
            match chosen.measure {
                Measure::Count(n) => out.count = Some(n),
                Measure::TotalPower(n) => out.total_power = Some(n),
                Measure::ForGeneric(n) => out.for_generic = Some(n),
                // No printed cost has it: convoke's part is the rules', not a face's.
                Measure::ForMana => {}
            }
            out
        }
        CostPart::Loyalty(n) => PartFacts {
            action: "Loyalty".to_owned(),
            amount: Some(i16::from(n)),
            ..PartFacts::default()
        },
        CostPart::Life(n) => PartFacts {
            action: "Life".to_owned(),
            amount: Some(i16::from(n)),
            ..PartFacts::default()
        },
        CostPart::DiscardHand => PartFacts {
            action: "DiscardHand".to_owned(),
            ..PartFacts::default()
        },
        CostPart::OneOf(costs) => PartFacts {
            action: "OneOf".to_owned(),
            one_of: costs
                .iter()
                .map(|cost| {
                    cost_facts(cost).unwrap_or(CostFacts {
                        mana: None,
                        parts: Vec::new(),
                    })
                })
                .collect(),
            ..PartFacts::default()
        },
    }
}

fn casting_facts(bit: usize, option: CastingOption) -> CastingFacts {
    CastingFacts {
        bit,
        keyword: match option {
            CastingOption::Fuse => "Fuse".to_owned(),
            CastingOption::Morph(_) => "Morph".to_owned(),
            CastingOption::Prototype { .. } => "Prototype".to_owned(),
            _ => variant(&option.kind().expect("a casting cost kind")),
        },
        alternative: option.is_alternative(),
        cost: CostFacts {
            mana: mana_text(option.cost()),
            parts: option.parts().iter().map(part_facts).collect(),
        },
        turn_up: if let CastingOption::Morph(cost) = option {
            cost_facts(&cost)
        } else {
            None
        },
        ability: match option {
            CastingOption::Overload { ability, .. } => Some(format!("Own {}", ability.get())),
            _ => None,
        },
    }
}

fn target_facts(spec: &TargetSpec) -> TargetFacts {
    TargetFacts {
        kind: variant(&spec.kind),
        count: spec.count,
        optional: spec.optional,
        other: spec.other,
    }
}

const COLORS: [(ManaColor, &str); 6] = [
    (ManaColor::White, "W"),
    (ManaColor::Blue, "U"),
    (ManaColor::Black, "B"),
    (ManaColor::Red, "R"),
    (ManaColor::Green, "G"),
    (ManaColor::Colorless, "C"),
];

fn mana_facts(mana: ManaSet) -> ManaFacts {
    ManaFacts {
        colors: COLORS
            .iter()
            .filter(|(color, _)| mana.includes(*color))
            .map(|(color, _)| variant(color))
            .collect(),
        amount: mana.amount,
    }
}

/// A mana cost in Scryfall's notation, as "{X}{2}{W}{U}" or "{1}{W/B}{W/B}", as the card
/// renderer writes it; `None` for no mana at all.
fn mana_text(cost: ManaCost) -> Option<String> {
    (!cost.is_zero()).then(|| beryl_card::render::mana_text(cost))
}

/// The name of a value's enum variant, from its `Debug` form.
fn variant(value: &impl Debug) -> String {
    let debug = format!("{value:?}");
    let end = debug
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(debug.len());
    debug[..end].to_owned()
}
