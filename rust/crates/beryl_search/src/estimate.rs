//! A card's value estimated from what its definition says it does (plan K9: "Generic agents
//! derive card knowledge from definitions"), for every card the per-deck overlay
//! ([`crate::overlay`]) has no hand-tuned value for. It is what [`crate::Knowledge::Estimate`]
//! knows of every card, and so what the AI knows of a deck nobody has tuned for.
//!
//! The estimate counts a few things a player looks at ([`CardFeatures`]) and weighs each by
//! a fixed constant. The weights were fitted once, by least squares, to the overlay's values
//! for the 47 cards it tuned (bead by-w8gv.15.3 records the fit), and are in the overlay's
//! units: a basic land about 10, a game-winning planeswalker about 90. They are never tuned
//! to a match result.
//!
//! It is a pure function of the definition, read once per game when [`crate::CardValues`] is
//! built, never in the rollout loop.

use std::cell::Cell;

use beryl_engine::Zone;
use beryl_engine::card::{
    AbilityKind, Amount, Behavior, CardDef, Effect, Modification, Objects, Players, Recipients,
    Taken, TriggerEvent, Types,
};

/// What [`estimate`] counts of a card. Each field is a whole number of the thing it names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CardFeatures {
    /// The front face's mana value.
    pub mana_value: i16,
    /// A creature front face's power plus toughness.
    pub body: i16,
    /// The front face's keyword abilities, such as flying and flash.
    pub keywords: i16,
    /// A planeswalker front face's starting loyalty.
    pub loyalty: i16,
    /// 1 for a land, which is played rather than cast.
    pub land: i16,
    /// Cards its controller draws, takes from the top of their library, finds by searching or
    /// manifests.
    pub cards: i16,
    /// Creatures it puts onto the battlefield under its controller's control as tokens or
    /// token copies, counting a token's power and toughness: a 2/2 counts 4.
    pub tokens: i16,
    /// Permanents it removes or neutralises: destroys, exiles, bounces, puts into a library,
    /// has sacrificed, steals, deals damage to or shrinks. One that does this to every
    /// permanent a filter accepts counts as [`SWEEP`] targets.
    pub removal: i16,
    /// Spells it counters.
    pub counters: i16,
    /// Mana it adds in one activation of its best mana ability.
    pub mana: i16,
    /// Abilities usable again and again: activated abilities (loyalty abilities among them)
    /// other than mana abilities, and triggers at the beginning of a step or phase.
    pub repeatable: i16,
    /// Other ways to cast it, such as evoke, overload or kicker.
    pub options: i16,
    /// Additional costs it must be cast with.
    pub extra_costs: i16,
}

/// How many single targets a removal of every permanent a filter accepts counts as.
pub(crate) const SWEEP: i16 = 2;

/// The weights of [`CardFeatures`], in hundredths of a value point, in the order of the
/// fields.
///
/// Fitted on 2026-11-12 by least squares to the overlay's 46 hand-tuned values (bead
/// by-w8gv.15.3; the data is printed by the ignored test `overlay_features`), with every
/// weight but [`BASE`] and [`LAND`] held at zero or above, and no weight for a feature that
/// fewer than two of those cards have (`extra_costs`). The fit's rank correlation with the
/// overlay is 0.885 (Spearman), against 0.719 for the estimate from mana value, stats and
/// loyalty that came before it. A weight of zero means the fit found nothing in the tuned
/// cards to price that feature by: their cards drawn, mana and other casting options are
/// already paid for in their mana value.
mod weight {
    pub const BASE: i32 = 2461;
    pub const MANA_VALUE: i32 = 460;
    pub const BODY: i32 = 108;
    pub const KEYWORDS: i32 = 493;
    pub const LOYALTY: i32 = 672;
    pub const LAND: i32 = -1385;
    pub const CARDS: i32 = 0;
    pub const TOKENS: i32 = 83;
    pub const REMOVAL: i32 = 973;
    pub const COUNTERS: i32 = 1586;
    pub const MANA: i32 = 0;
    pub const REPEATABLE: i32 = 97;
    pub const OPTIONS: i32 = 0;
    pub const EXTRA_COSTS: i32 = 0;
}

/// The card's estimated value, from what its definition says it does.
pub(crate) fn estimate(def: &CardDef) -> i16 {
    let f = features(def);
    let hundredths = weight::BASE
        + weight::MANA_VALUE * i32::from(f.mana_value)
        + weight::BODY * i32::from(f.body)
        + weight::KEYWORDS * i32::from(f.keywords)
        + weight::LOYALTY * i32::from(f.loyalty)
        + weight::LAND * i32::from(f.land)
        + weight::CARDS * i32::from(f.cards)
        + weight::TOKENS * i32::from(f.tokens)
        + weight::REMOVAL * i32::from(f.removal)
        + weight::COUNTERS * i32::from(f.counters)
        + weight::MANA * i32::from(f.mana)
        + weight::REPEATABLE * i32::from(f.repeatable)
        + weight::OPTIONS * i32::from(f.options)
        + weight::EXTRA_COSTS * i32::from(f.extra_costs);
    i16::try_from((hundredths / 100).clamp(0, i32::from(i16::MAX))).unwrap_or(i16::MAX)
}

/// What [`estimate`] counts of `def`. Stats come from the front face; abilities from every
/// face, since a transforming card's back face is part of what it does.
///
/// A card with several spell abilities, such as a spell with an overload ability, does one of
/// them when cast, so each count takes the most any one of them gives; every other ability
/// adds to the counts.
pub(crate) fn features(def: &CardDef) -> CardFeatures {
    let front = def.front();
    let is = |types: Types| front.types.intersects(types);
    let mut f = CardFeatures {
        mana_value: i16::from(front.cost.mana_value()),
        keywords: i16::try_from(front.keywords.0.count_ones()).unwrap_or(0),
        land: i16::from(is(Types::LAND)),
        options: i16::try_from(front.casting.len()).unwrap_or(0),
        extra_costs: i16::try_from(front.additional_cost.len()).unwrap_or(0),
        ..CardFeatures::default()
    };
    if is(Types::CREATURE) {
        f.body = i16::from(front.power.unwrap_or(0)) + i16::from(front.toughness.unwrap_or(0));
    }
    if is(Types::PLANESWALKER) {
        f.loyalty = i16::from(front.loyalty.unwrap_or(0));
    }
    let mut spell = Effects::default();
    let mut others = Effects::default();
    for a in def.faces.iter().flat_map(|face| face.abilities) {
        match a.kind() {
            AbilityKind::Mana => f.mana = f.mana.max(i16::from(a.produces().amount)),
            AbilityKind::Activated => f.repeatable += 1,
            AbilityKind::Triggered if a.trigger().is_some_and(|t| recurs(t.event)) => {
                f.repeatable += 1;
            }
            _ => {}
        }
        let Behavior::Data(effect) = a.body() else {
            continue;
        };
        let counted = Effects::of(effect);
        if a.kind() == AbilityKind::Spell {
            spell = spell.most(counted);
        } else if a.kind() != AbilityKind::Mana {
            others = others.plus(counted);
        }
    }
    let effects = spell.plus(others);
    f.cards = effects.cards;
    f.tokens = effects.tokens;
    f.removal = effects.removal;
    f.counters = effects.counters;
    f
}

/// Whether a trigger on `event` happens again every turn.
fn recurs(event: TriggerEvent) -> bool {
    matches!(event, TriggerEvent::Step(_) | TriggerEvent::MainPhase)
}

/// The counts one ability's effect gives.
#[derive(Clone, Copy, Debug, Default)]
struct Effects {
    cards: i16,
    tokens: i16,
    removal: i16,
    counters: i16,
}

impl Effects {
    /// What the instructions of `effect` add up to, every branch of it counted.
    fn of(effect: &Effect) -> Effects {
        let counts = Cell::new(Effects::default());
        effect.any(&|instruction| {
            counts.set(counts.get().plus(Self::instruction(instruction)));
            false
        });
        counts.get()
    }

    /// What one instruction counts, not looking inside it.
    fn instruction(instruction: &Effect) -> Effects {
        let mut e = Effects::default();
        match *instruction {
            Effect::Draw {
                player: Players::You,
                count,
            }
            | Effect::Search {
                player: Players::You,
                count,
                ..
            } => e.cards = amount(count),
            Effect::Dig {
                player: Players::You,
                take,
                ..
            } => {
                e.cards = match take {
                    Taken::Put(n) => i16::from(n),
                    Taken::MayReveal(_) => 1,
                };
            }
            Effect::ManifestDread(Players::You) => e.cards = 1,
            Effect::CreateTokens {
                player: Players::You,
                token,
                count,
            }
            | Effect::CreateAttackingTokens {
                player: Players::You,
                token,
                count,
                ..
            } => e.tokens = amount(count) * creature_body(token),
            Effect::CreateCopies {
                player: Players::You,
                count,
                ..
            } => e.tokens = amount(count),
            Effect::Destroy(objects)
            | Effect::Exile(objects)
            | Effect::ExileUntilThisLeaves(objects)
            | Effect::ReturnToHand(objects)
            | Effect::Sacrifice(objects)
            | Effect::PutIntoLibrary { objects, .. }
            | Effect::DealDamage {
                to: Recipients::Permanents(objects),
                ..
            } => e.removal = removed(objects),
            Effect::Modify {
                objects,
                modification,
                ..
            } if hinders(modification) => e.removal = removed(objects),
            Effect::Counter(_) | Effect::CounterInto { .. } => e.counters = 1,
            _ => {}
        }
        e
    }

    fn plus(self, other: Effects) -> Effects {
        Effects {
            cards: self.cards + other.cards,
            tokens: self.tokens + other.tokens,
            removal: self.removal + other.removal,
            counters: self.counters + other.counters,
        }
    }

    fn most(self, other: Effects) -> Effects {
        Effects {
            cards: self.cards.max(other.cards),
            tokens: self.tokens.max(other.tokens),
            removal: self.removal.max(other.removal),
            counters: self.counters.max(other.counters),
        }
    }
}

/// How many an amount comes to: a fixed number as written, any other as 1, since what it
/// counts is only known in a game.
fn amount(count: Amount) -> i16 {
    match count {
        Amount::Fixed(n) => i16::from(n),
        Amount::Times(n, &inner) => i16::from(n) * amount(inner),
        _ => 1,
    }
}

/// A token's worth in [`CardFeatures::tokens`]: a creature token's power plus toughness, at
/// least 1, and 0 for a token that is no creature, such as a Treasure.
fn creature_body(token: &CardDef) -> i16 {
    let face = token.front();
    if face.types.intersects(Types::CREATURE) {
        (i16::from(face.power.unwrap_or(0)) + i16::from(face.toughness.unwrap_or(0))).max(1)
    } else {
        0
    }
}

/// How many permanents an instruction aimed at `objects` removes: a target counts 1, every
/// permanent a filter accepts counts [`SWEEP`], and the card itself, the objects an earlier
/// instruction acted on and cards in other zones count nothing.
fn removed(objects: Objects) -> i16 {
    match objects {
        Objects::Target(_) | Objects::AttachedTo => 1,
        Objects::All {
            zone: Zone::Battlefield,
            ..
        } => SWEEP,
        _ => 0,
    }
}

/// Whether a modification takes a permanent out of its controller's use: gaining control of
/// it, or giving it −N power or toughness.
fn hinders(modification: Modification) -> bool {
    match modification {
        Modification::Control => true,
        Modification::ModifyPt(power, toughness) => power < 0 || toughness < 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
