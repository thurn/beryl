//! Walking a definition: every [`Filter`], [`Amount`] and [`Condition`] it holds, wherever it
//! holds one, for a check that must see them all, and every [`Subtype`](crate::Subtype) it
//! names.

use crate::ability::{Ability, Behavior};
use crate::characteristics::Subtypes;
use crate::continuous::{Governed, LayerSpec, Modification};
use crate::cost::{Cost, CostPart};
use crate::definition::CardDef;
use crate::effect::{Amount, Condition, Effect, Objects, Players};
use crate::filter::Filter;
use crate::trigger::Subject;

/// A part of a definition a walk meets: each filter, amount and condition, those inside
/// another included.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Filter(&'a Filter),
    Amount(&'a Amount),
    Condition(&'a Condition),
}

/// What is done with each part met.
type Visit<'a> = &'a mut dyn FnMut(Node<'_>);

impl CardDef {
    /// Calls `visit` with every filter of the card's faces ([`CardDef::walk`]).
    pub fn filters(&self, visit: &mut dyn FnMut(&Filter)) {
        self.walk(&mut |node| {
            if let Node::Filter(filter) = node {
                visit(filter);
            }
        });
    }

    /// Calls `visit` with every filter, amount and condition of the card's faces: those of
    /// its abilities, of the abilities it grants and of the costs it is cast with, those
    /// inside them included. A token it makes is a card of its own ([`CardDef::tokens`]) and
    /// is not walked.
    pub fn walk(&self, visit: Visit) {
        for face in self.faces {
            for ability in face.abilities.iter().chain(face.grants) {
                ability.walk(visit);
            }
            let options = face.casting.iter().flat_map(|option| option.parts());
            for part in face.additional_cost.iter().chain(options) {
                part.walk(visit);
            }
        }
    }
}

impl CardDef {
    /// Calls `visit` with every set of subtypes an object of the card can have or a rule of
    /// it can ask about: each face's printed subtypes, each subtype a filter names
    /// ([`Filter::Subtype`]) and each a continuous effect gives
    /// ([`Modification::AddSubtype`]), in its static abilities or its effect data. The game
    /// numbers what this finds (`beryl_kernel`'s `GameTable`); a Rust body that names
    /// another subtype is read by nothing, so it names none a game does not.
    pub fn subtypes(&self, visit: &mut dyn FnMut(Subtypes)) {
        // "Islandwalk" asks whether a land the defending player controls is an Island
        // (CR 702.14c), so a landwalk names its subtype.
        let given = |modification: &Modification| match *modification {
            Modification::AddSubtype(subtype) => Some(Subtypes::of(&[subtype])),
            Modification::AddKeywords(keywords) => Some(keywords.landwalk()),
            _ => None,
        };
        for face in self.faces {
            visit(face.subtypes.union(face.keywords.landwalk()));
            for ability in face.abilities.iter().chain(face.grants) {
                if let Some(spec) = ability.layer() {
                    for given in spec.modifications.iter().filter_map(given) {
                        visit(given);
                    }
                }
                if let Behavior::Data(effect) = ability.body() {
                    let found = std::cell::RefCell::new(Subtypes::NONE);
                    effect.any(&|instruction| {
                        if let Effect::Modify { modification, .. } = instruction
                            && let Some(given) = given(modification)
                        {
                            let mut found = found.borrow_mut();
                            *found = found.union(given);
                        }
                        false
                    });
                    visit(found.into_inner());
                }
            }
        }
        let mut named = Subtypes::NONE;
        self.filters(&mut |filter| {
            if let Filter::Subtype(subtype) = *filter {
                named = named.with(subtype);
            }
        });
        visit(named);
    }
}

impl CardDef {
    /// Calls `visit` with every token and emblem the effect data of the card's faces
    /// creates ([`Effect::CreateTokens`], [`Effect::CreateAttackingTokens`],
    /// [`Effect::GetEmblem`]): what the card must list among what it creates
    /// ([`CardDef::tokens`]) for a game to have them.
    pub fn creates(&self, visit: &mut dyn FnMut(&'static CardDef)) {
        let visit = std::cell::RefCell::new(visit);
        for face in self.faces {
            for ability in face.abilities.iter().chain(face.grants) {
                let Behavior::Data(effect) = ability.body() else {
                    continue;
                };
                effect.any(&|instruction| {
                    if let Effect::CreateTokens { token: made, .. }
                    | Effect::CreateAttackingTokens { token: made, .. }
                    | Effect::GetEmblem { emblem: made, .. } = *instruction
                    {
                        (visit.borrow_mut())(made);
                    }
                    false
                });
            }
        }
    }
}

impl Ability {
    /// Calls `visit` with every filter of the ability ([`Ability::walk`]).
    pub fn filters(&self, visit: &mut dyn FnMut(&Filter)) {
        self.walk(&mut |node| {
            if let Node::Filter(filter) = node {
                visit(filter);
            }
        });
    }

    /// Calls `visit` with every filter, amount and condition of the ability: what it may
    /// target, what pays its cost, what triggers it, what it needs to be activated, its
    /// conditions, its continuous effect, the rule change it names and its effect data. A
    /// Rust body holds none anything can read.
    pub fn walk(&self, visit: Visit) {
        for spec in self.targets() {
            spec.filter.walk(visit);
        }
        for part in self.cost().parts() {
            part.walk(visit);
        }
        if let Some(trigger) = self.trigger() {
            if let Subject::A(filter) = &trigger.subject {
                filter.walk(visit);
            }
            trigger.whose.walk(visit);
        }
        if let Some(condition) = self.condition() {
            condition.walk(visit);
        }
        if let Some(layer) = self.layer() {
            layer.walk(visit);
        }
        if let Some(rule) = self.rule() {
            rule.players.walk(visit);
            match &rule.what {
                Governed::CastSpells(filter) => filter.walk(visit),
                Governed::ActivateLoyaltyAbilities(objects) => objects.walk(visit),
            }
        }
        if let Some(filter) = &self.only_if_you_control() {
            filter.walk(visit);
        }
        if let Behavior::Data(effect) = self.body() {
            effect.walk(visit);
        }
    }
}

impl CostPart {
    fn walk(&self, visit: Visit) {
        match self {
            CostPart::This(_) | CostPart::Loyalty(_) | CostPart::Life(_) => {}
            CostPart::Chosen(_, chosen) => chosen.filter.walk(visit),
            CostPart::OneOf(costs) => {
                for part in costs.iter().flat_map(Cost::parts) {
                    part.walk(visit);
                }
            }
        }
    }
}

impl LayerSpec {
    fn walk(&self, visit: Visit) {
        self.affected.walk(visit);
        if let Some(condition) = &self.condition {
            condition.walk(visit);
        }
        for modification in self.modifications {
            modification.walk(visit);
        }
    }
}

impl Modification {
    fn walk(&self, visit: Visit) {
        match self {
            Modification::SetPower(amount) => amount.walk(visit),
            Modification::SetBasePt(power, toughness) => {
                power.walk(visit);
                toughness.walk(visit);
            }
            _ => {}
        }
    }
}

impl Filter {
    /// Calls `visit` with this filter and then with every part inside it.
    pub fn walk(&self, visit: Visit) {
        visit(Node::Filter(self));
        match self {
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::Subtype(_)
            | Filter::Colors(_)
            | Filter::Tapped
            | Filter::ManaValueAtMost(_)
            | Filter::ManaValueAtMostX => {}
            Filter::Whose(players) => players.walk(visit),
            Filter::Not(filter) => filter.walk(visit),
            Filter::All(filters) | Filter::AnyOf(filters) => {
                for filter in *filters {
                    filter.walk(visit);
                }
            }
        }
    }
}

impl Objects {
    fn walk(&self, visit: Visit) {
        match self {
            Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::Paid(_)
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::ExiledWithThis => {}
            Objects::All { filter, .. } => filter.walk(visit),
            Objects::InZoneOf {
                players, filter, ..
            } => {
                players.walk(visit);
                filter.walk(visit);
            }
            Objects::CardsIn { players, .. } => players.walk(visit),
        }
    }
}

impl Players {
    fn walk(&self, visit: Visit) {
        match self {
            Players::You
            | Players::EachOpponent
            | Players::EachPlayer
            | Players::Target(_)
            | Players::ThatPlayer => {}
            Players::ControllerOf(objects) | Players::OwnerOf(objects) => objects.walk(visit),
            Players::EachOther(players) => players.walk(visit),
        }
    }
}

impl Amount {
    /// Calls `visit` with this amount and then with every part inside it.
    pub fn walk(&self, visit: Visit) {
        visit(Node::Amount(self));
        match self {
            Amount::Fixed(_) | Amount::X | Amount::ThatMany | Amount::Carried => {}
            Amount::Count(objects) => objects.walk(visit),
            Amount::Times(_, amount) => amount.walk(visit),
            Amount::Life(players)
            | Amount::ThoseOf(players)
            | Amount::SpellsCast(players)
            | Amount::CardsDrawn(players) => players.walk(visit),
            Amount::Most(players, amount) => {
                players.walk(visit);
                amount.walk(visit);
            }
            Amount::Power(objects)
            | Amount::Toughness(objects)
            | Amount::ManaValue(objects)
            | Amount::Counters(objects, _) => objects.walk(visit),
        }
    }
}

impl Condition {
    /// Calls `visit` with this condition and then with every part inside it.
    pub fn walk(&self, visit: Visit) {
        visit(Node::Condition(self));
        match self {
            Condition::Did | Condition::WonFlip | Condition::Paid(_) => {}
            Condition::DrewInDrawStep(players) => players.walk(visit),
            Condition::Compare(left, _, right) => {
                left.walk(visit);
                right.walk(visit);
            }
            Condition::Is(objects, filter) => {
                objects.walk(visit);
                filter.walk(visit);
            }
            Condition::EnteredThisTurn(objects) | Condition::Token(objects) => {
                objects.walk(visit);
            }
            Condition::Not(condition) => condition.walk(visit),
            Condition::All(conditions) | Condition::Any(conditions) => {
                for condition in *conditions {
                    condition.walk(visit);
                }
            }
        }
    }
}

impl Effect {
    fn walk(&self, visit: Visit) {
        let operands = self.operands();
        for players in [operands.players, operands.others].into_iter().flatten() {
            players.walk(visit);
        }
        for objects in [operands.objects, operands.to].into_iter().flatten() {
            objects.walk(visit);
        }
        if let Some(amount) = operands.amount {
            amount.walk(visit);
        }
        if let Some(filter) = operands.filter {
            filter.walk(visit);
        }
        match self {
            Effect::Sequence(effects) => {
                for effect in *effects {
                    effect.walk(visit);
                }
            }
            Effect::If {
                condition,
                then,
                otherwise,
            } => {
                condition.walk(visit);
                then.walk(visit);
                if let Some(otherwise) = otherwise {
                    otherwise.walk(visit);
                }
            }
            Effect::May { effect, .. }
            | Effect::Unless { effect, .. }
            | Effect::ForEachPlayer { effect, .. } => effect.walk(visit),
            Effect::Modify { modification, .. } => modification.walk(visit),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characteristics::Types;
    use crate::effect::Comparison;
    use crate::ids::Zone;
    use crate::target::{TargetKind, TargetSpec};
    use crate::trigger::Trigger;

    /// "Whenever an opponent casts a noncreature spell, if you control an artifact, destroy
    /// target tapped land": a filter in each place an ability holds one, some nested.
    static ABILITY: Ability = Ability::triggered(&Trigger::casts(
        Players::EachOpponent,
        Filter::Not(&Filter::Types(Types::CREATURE)),
    ))
    .effect(&crate::effect::build::destroy(Objects::All {
        zone: Zone::Battlefield,
        filter: Filter::ManaValueAtMost(2),
    }))
    .targeting(&[TargetSpec::one(
        TargetKind::Permanent,
        Filter::All(&[Filter::Tapped, Filter::Types(Types::LAND)]),
    )])
    .only_if(&Condition::Compare(
        Amount::Count(&Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Whose(&Players::You),
        }),
        Comparison::AtLeast,
        Amount::Fixed(1),
    ));

    #[test]
    fn a_walk_meets_every_filter_of_an_ability_and_those_inside_them() {
        let mut met = Vec::new();
        ABILITY.filters(&mut |filter| met.push(format!("{filter:?}")));
        let outer = |debug: &str| debug.split('(').next().unwrap().to_owned();
        assert_eq!(
            met.iter().map(|debug| outer(debug)).collect::<Vec<_>>(),
            [
                "All",
                "Tapped",
                "Types",
                "Not",
                "Types",
                "Whose",
                "ManaValueAtMost"
            ]
        );
    }
}
