//! Walking a definition: every [`Filter`] it holds, wherever it holds one, for a check that
//! must see them all.

use crate::ability::{Ability, Behavior};
use crate::continuous::{Governed, LayerSpec, Modification};
use crate::cost::{Cost, CostPart};
use crate::definition::CardDef;
use crate::effect::{Amount, Condition, Effect, Objects, Players, Recipients};
use crate::filter::Filter;
use crate::trigger::Subject;

/// What is done with each filter.
type Visit<'a> = &'a mut dyn FnMut(&Filter);

impl CardDef {
    /// Calls `visit` with every filter of the card's faces: those of its abilities, of the
    /// abilities it grants and of the costs it is cast with, the filters inside them included. A token it makes is a card of
    /// its own ([`CardDef::tokens`]) and is not walked.
    pub fn filters(&self, visit: Visit) {
        for face in self.faces {
            for ability in face.abilities.iter().chain(face.grants) {
                ability.filters(visit);
            }
            let options = face.casting.iter().flat_map(|option| option.parts());
            for part in face.additional_cost.iter().chain(options) {
                part.filters(visit);
            }
        }
    }
}

impl Ability {
    /// Calls `visit` with every filter of the ability: what it may target, what pays its
    /// cost, what triggers it, what it needs to be activated, its conditions, its continuous
    /// effect, the rule change it names and its effect data. A Rust body holds none anything can read.
    pub fn filters(&self, visit: Visit) {
        for spec in self.targets() {
            spec.filter.filters(visit);
        }
        for part in self.cost().parts() {
            part.filters(visit);
        }
        if let Some(trigger) = self.trigger() {
            if let Subject::A(filter) = &trigger.subject {
                filter.filters(visit);
            }
            trigger.whose.filters(visit);
        }
        if let Some(condition) = self.condition() {
            condition.filters(visit);
        }
        if let Some(layer) = self.layer() {
            layer.filters(visit);
        }
        if let Some(rule) = self.rule() {
            rule.players.filters(visit);
            match &rule.what {
                Governed::CastSpells(filter) => filter.filters(visit),
                Governed::ActivateLoyaltyAbilities(objects) => objects.filters(visit),
            }
        }
        if let Some(filter) = &self.only_if_you_control() {
            filter.filters(visit);
        }
        if let Behavior::Data(effect) = self.body() {
            effect.filters(visit);
        }
    }
}

impl CostPart {
    fn filters(&self, visit: Visit) {
        match self {
            CostPart::This(_) | CostPart::Loyalty(_) | CostPart::Life(_) => {}
            CostPart::Chosen(_, chosen) => chosen.filter.filters(visit),
            CostPart::OneOf(costs) => {
                for part in costs.iter().flat_map(Cost::parts) {
                    part.filters(visit);
                }
            }
        }
    }
}

impl LayerSpec {
    fn filters(&self, visit: Visit) {
        self.affected.filters(visit);
        if let Some(condition) = &self.condition {
            condition.filters(visit);
        }
        self.modification.filters(visit);
    }
}

impl Modification {
    fn filters(&self, visit: Visit) {
        if let Modification::SetPower(amount) = self {
            amount.filters(visit);
        }
    }
}

impl Filter {
    /// Calls `visit` with this filter and then with every filter inside it.
    pub fn filters(&self, visit: Visit) {
        visit(self);
        match self {
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::Subtype(_)
            | Filter::Colors(_)
            | Filter::Tapped
            | Filter::ManaValueAtMost(_)
            | Filter::ManaValueAtMostX => {}
            Filter::Whose(players) => players.filters(visit),
            Filter::Not(filter) => filter.filters(visit),
            Filter::All(filters) | Filter::AnyOf(filters) => {
                for filter in *filters {
                    filter.filters(visit);
                }
            }
        }
    }
}

impl Objects {
    fn filters(&self, visit: Visit) {
        match self {
            Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::Paid(_)
            | Objects::TriggeredOn
            | Objects::ExiledWithThis => {}
            Objects::All { filter, .. } => filter.filters(visit),
            Objects::InZoneOf {
                players, filter, ..
            } => {
                players.filters(visit);
                filter.filters(visit);
            }
        }
    }
}

impl Players {
    fn filters(&self, visit: Visit) {
        match self {
            Players::You
            | Players::EachOpponent
            | Players::EachPlayer
            | Players::Target(_)
            | Players::ThatPlayer => {}
            Players::ControllerOf(objects) | Players::OwnerOf(objects) => objects.filters(visit),
            Players::EachOther(players) => players.filters(visit),
        }
    }
}

impl Recipients {
    fn filters(&self, visit: Visit) {
        match self {
            Recipients::Players(players) => players.filters(visit),
            Recipients::Permanents(objects) => objects.filters(visit),
        }
    }
}

impl Amount {
    fn filters(&self, visit: Visit) {
        match self {
            Amount::Fixed(_) | Amount::X | Amount::ThatMany => {}
            Amount::Count(objects) => objects.filters(visit),
            Amount::Times(_, amount) => amount.filters(visit),
            Amount::Life(players) => players.filters(visit),
            Amount::Most(players, amount) => {
                players.filters(visit);
                amount.filters(visit);
            }
            Amount::Power(objects)
            | Amount::Toughness(objects)
            | Amount::ManaValue(objects)
            | Amount::Counters(objects, _) => objects.filters(visit),
        }
    }
}

impl Condition {
    fn filters(&self, visit: Visit) {
        match self {
            Condition::Did | Condition::WonFlip | Condition::Paid(_) | Condition::Custom(_) => {}
            Condition::Compare(left, _, right) => {
                left.filters(visit);
                right.filters(visit);
            }
            Condition::Is(objects, filter) => {
                objects.filters(visit);
                filter.filters(visit);
            }
            Condition::EnteredThisTurn(objects) | Condition::Token(objects) => {
                objects.filters(visit);
            }
            Condition::Not(condition) => condition.filters(visit),
            Condition::All(conditions) | Condition::Any(conditions) => {
                for condition in *conditions {
                    condition.filters(visit);
                }
            }
        }
    }
}

impl Effect {
    fn filters(&self, visit: Visit) {
        match self {
            Effect::Sequence(effects) => {
                for effect in *effects {
                    effect.filters(visit);
                }
            }
            Effect::If {
                condition,
                then,
                otherwise,
            } => {
                condition.filters(visit);
                then.filters(visit);
                if let Some(otherwise) = otherwise {
                    otherwise.filters(visit);
                }
            }
            Effect::May { player, effect }
            | Effect::Unless { effect, player, .. }
            | Effect::ForEachPlayer {
                players: player,
                effect,
            } => {
                player.filters(visit);
                effect.filters(visit);
            }
            _ => self.operands(visit),
        }
    }

    /// The filters of one instruction's operands, where it holds no other instruction.
    fn operands(&self, visit: Visit) {
        match self {
            // Those that hold others, which `filters` takes apart, and a rule change, whose
            // rule is the ability's, walked with it (`Ability::rule`).
            Effect::Sequence(_)
            | Effect::If { .. }
            | Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::ForEachPlayer { .. }
            | Effect::ChangeRule { .. } => {}
            Effect::Counter(objects)
            | Effect::Destroy(objects)
            | Effect::Exile(objects)
            | Effect::ExileUntilThisLeaves(objects)
            | Effect::ExileLinked(objects)
            | Effect::Sacrifice(objects)
            | Effect::ReturnToHand(objects)
            | Effect::PutIntoGraveyard(objects)
            | Effect::ReturnTransformed(objects)
            | Effect::PutIntoLibrary { objects, .. }
            | Effect::Explore(objects)
            | Effect::GainFlashback(objects)
            | Effect::CounterInto {
                spells: objects, ..
            } => objects.filters(visit),
            Effect::DealDamage { to, amount } => {
                to.filters(visit);
                amount.filters(visit);
            }
            Effect::MayCast { player, card, .. }
            | Effect::PermitCast {
                player,
                cards: card,
                ..
            }
            | Effect::LookAt {
                player,
                cards: card,
            }
            | Effect::Choose {
                player,
                among: card,
                ..
            } => {
                player.filters(visit);
                card.filters(visit);
            }
            Effect::CreateAttackingTokens {
                player,
                count,
                attacking,
                ..
            } => {
                player.filters(visit);
                count.filters(visit);
                attacking.filters(visit);
            }
            Effect::Modify {
                objects,
                modification,
                ..
            } => {
                objects.filters(visit);
                modification.filters(visit);
            }
            Effect::Draw {
                player,
                count: amount,
            }
            | Effect::Discard {
                player,
                count: amount,
            }
            | Effect::Mill {
                player,
                count: amount,
            }
            | Effect::Scry {
                player,
                count: amount,
            }
            | Effect::GainLife { player, amount }
            | Effect::LoseLife { player, amount }
            | Effect::CreateTokens {
                player,
                count: amount,
                ..
            } => {
                player.filters(visit);
                amount.filters(visit);
            }
            Effect::CreateCopies {
                player, of, count, ..
            } => {
                player.filters(visit);
                of.filters(visit);
                count.filters(visit);
            }
            Effect::AddCounters { objects, count, .. }
            | Effect::RemoveCounters { objects, count, .. } => {
                objects.filters(visit);
                count.filters(visit);
            }
            Effect::UntapUpTo {
                player,
                count,
                permanents,
            } => {
                player.filters(visit);
                count.filters(visit);
                permanents.filters(visit);
            }
            // The delayed ability is one of the face's, walked with them.
            Effect::Delay { carrying, .. } => carrying.filters(visit),
            Effect::AddMana { player, .. }
            | Effect::GetEmblem { player, .. }
            | Effect::ManifestDread(player)
            | Effect::FlipCoin(player)
            | Effect::WinGame(player)
            | Effect::LoseGame(player) => {
                player.filters(visit);
            }
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
    .effect(&Effect::Destroy(Objects::All {
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
