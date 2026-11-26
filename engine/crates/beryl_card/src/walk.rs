//! Walking a definition: every [`Filter`], [`Amount`], [`Condition`] and [`Objects`] it
//! holds, wherever it holds one, for a check that must see them all, and every
//! [`Subtype`](crate::Subtype) it names.

use crate::ability::{Ability, Behavior};
use crate::characteristics::{BasicLandTypes, Subtypes};
use crate::continuous::{Governed, LayerSpec, Modification, Replaced};
use crate::cost::{Cost, CostPart};
use crate::definition::CardDef;
use crate::effect::{Amount, Branches, Condition, Effect, Objects, Players};
use crate::filter::Filter;
use crate::trigger::Subject;
use crate::words::Words;

/// A part of a definition a walk meets: each filter, amount, condition and object
/// expression, those inside another included.
#[derive(Clone, Copy, Debug)]
pub enum Node<'a> {
    Filter(&'a Filter),
    Amount(&'a Amount),
    Condition(&'a Condition),
    Objects(&'a Objects),
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
    /// ([`Filter::SubtypeWord`]) and each a continuous effect gives
    /// ([`Modification::AddSubtypeWord`], [`Modification::SetSubtypeWord`],
    /// [`Modification::SetLandTypeWords`]), in its static abilities or its effect data. The
    /// game numbers what this finds (`beryl_kernel`'s `GameTable`); a Rust body that names
    /// another subtype is read by nothing, so it names none a game does not.
    ///
    /// These are the words as printed ([`Words::NONE`]). A subtype that a text change makes
    /// a word read as is named by the card that changes the text: an effect that replaces
    /// one basic land type with another ([`Effect::ChangeText`]) names all five.
    pub fn subtypes(&self, visit: &mut dyn FnMut(Subtypes)) {
        let printed = &Words::NONE;
        for face in self.faces {
            visit(face.subtypes.read(printed).union(face.keywords.landwalk()));
            // A copy exception's "is an Illusion in addition to its other types" (CR 707.9b).
            for change in face.excepts.iter().flat_map(|exception| exception.0) {
                if let crate::CopyChange::AddSubtype(subtype) = *change {
                    visit(Subtypes::of(&[subtype]));
                }
            }
            for ability in face.abilities.iter().chain(face.grants) {
                if let Some(spec) = ability.layer() {
                    for given in spec
                        .modifications
                        .iter()
                        .filter_map(Modification::granted_subtypes)
                    {
                        visit(given);
                    }
                }
                if let Behavior::Data(effect) = ability.body() {
                    visit(effect.subtypes());
                }
            }
        }
        let mut named = Subtypes::NONE;
        self.filters(&mut |filter| {
            if let Filter::SubtypeWord(word) = *filter {
                named = named.with(word.read(printed));
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

/// Which of an ability's filters a question asks with: the place of the first filter equal to
/// it among those [`Ability::filters`] meets. A decision carries it so that whoever shows the
/// question can say what it takes ("Search for an artifact card with mana value 1 or less.")
/// in one byte, where the filter itself would not fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FilterIx(pub u8);

impl Ability {
    /// The place of `filter` among the ability's filters, if the ability has it and it says
    /// more than "a card" ([`Filter::Any`]).
    pub fn filter_index(&self, filter: Filter) -> Option<FilterIx> {
        if filter == Filter::Any {
            return None;
        }
        let (mut at, mut found) = (0u8, None);
        self.filters(&mut |each| {
            if found.is_none() && *each == filter {
                found = Some(FilterIx(at));
            }
            at = at.saturating_add(1);
        });
        found.filter(|ix| ix.0 < u8::MAX)
    }

    /// The filter at `ix` among the ability's filters ([`Ability::filter_index`]).
    pub fn filter_at(&self, ix: FilterIx) -> Option<Filter> {
        let (mut at, mut found) = (0u8, None);
        self.filters(&mut |each| {
            if at == ix.0 && found.is_none() {
                found = Some(*each);
            }
            at = at.saturating_add(1);
        });
        found
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
    /// conditions, its continuous effect, what it replaces, the rule change it names, the
    /// amounts of what a mana ability makes and its effect data. A Rust body holds none
    /// anything can read.
    pub fn walk(&self, visit: Visit) {
        for spec in self.targets() {
            spec.filter.walk(visit);
        }
        for part in self.cost().parts() {
            part.walk(visit);
        }
        // What a mana ability makes: an amount a term counts.
        for term in self.product() {
            term.amount.walk(visit);
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
        match self.replacement().map(|r| &r.replaces) {
            Some(Replaced::Draw(players, _)) => players.walk(visit),
            Some(
                Replaced::PutInto(filter, _)
                | Replaced::Enters {
                    objects: filter, ..
                },
            ) => {
                filter.walk(visit);
            }
            Some(
                Replaced::None | Replaced::ThisEnters | Replaced::BeginTurn | Replaced::Damage,
            )
            | None => {}
        }
        if let Some(rule) = self.rule() {
            rule.players.walk(visit);
            match &rule.what {
                Governed::CastSpells(filter)
                | Governed::SpellCosts { spells: filter, .. }
                | Governed::SpendAsAnyColor(filter) => {
                    filter.walk(visit);
                }
                Governed::ActivateLoyaltyAbilities(objects)
                | Governed::Block(objects)
                | Governed::UntapInUntapStep(objects)
                | Governed::BlockAnyNumber(objects)
                | Governed::Counter(objects) => {
                    objects.walk(visit);
                }
                Governed::Attack {
                    attackers,
                    unless_defender_controls,
                } => {
                    attackers.walk(visit);
                    if let Some(filter) = unless_defender_controls {
                        filter.walk(visit);
                    }
                }
                Governed::ActivateAbilities { .. }
                | Governed::Player(_)
                | Governed::PreventDamage
                | Governed::TakeStep(_) => {}
            }
        }
        if let Some(filter) = &self.only_if_you_control() {
            filter.walk(visit);
        }
        if let Some(restriction) = self.spend() {
            restriction.cast.walk(visit);
            if let Some(rider) = restriction.rider {
                rider.players.walk(visit);
            }
        }
        if let Behavior::Data(effect) = self.body() {
            effect.walk(visit);
        }
    }
}

impl CostPart {
    fn walk(&self, visit: Visit) {
        match self {
            CostPart::This(_)
            | CostPart::Loyalty(_)
            | CostPart::Life(_)
            | CostPart::DiscardHand => {}
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
        if let Some((power, toughness)) = self.pt_amounts() {
            power.walk(visit);
            if let Some(toughness) = toughness {
                toughness.walk(visit);
            }
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
            | Filter::SubtypeWord(_)
            | Filter::ColorWords(_)
            | Filter::AttackedThisTurn
            | Filter::Tapped
            | Filter::Token
            | Filter::Named(_)
            | Filter::ManaValueAtMost(_)
            | Filter::PowerAtMost(_)
            | Filter::ManaValueAtMostX
            | Filter::ManaValueAtMostPower
            | Filter::InZone(_) => {}
            Filter::Whose(players) | Filter::Owned(players) => players.walk(visit),
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
        visit(Node::Objects(self));
        match self {
            Objects::This
            | Objects::Target(_)
            | Objects::AttachedTo
            | Objects::Those
            | Objects::Paid(_)
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::Carried
            | Objects::ExiledWithThis
            | Objects::PutOntoBattlefieldWithThis => {}
            Objects::All { filter, .. } | Objects::Everywhere(filter) => filter.walk(visit),
            Objects::InZoneOf {
                players, filter, ..
            } => {
                players.walk(visit);
                filter.walk(visit);
            }
            Objects::CardsIn { players, .. } | Objects::LibraryTop { players, .. } => {
                players.walk(visit);
            }
            Objects::Other(objects) | Objects::ExceptThose(objects) => objects.walk(visit),
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
            | Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::Enchanted => {}
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
            Amount::Fixed(_)
            | Amount::X
            | Amount::ThatMany
            | Amount::Carried
            | Amount::SpellsCastBefore => {}
            Amount::Count(objects) | Amount::CardTypesAmong(objects) => objects.walk(visit),
            Amount::Times(_, amount)
            | Amount::Plus(_, amount)
            | Amount::Half { of: amount, .. } => amount.walk(visit),
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
            Condition::DrewInDrawStep(players)
            | Condition::TurnOf(players)
            | Condition::AttackedThisTurn(players) => players.walk(visit),
            Condition::Compare(left, _, right) => {
                left.walk(visit);
                right.walk(visit);
            }
            Condition::Is(objects, filter) => {
                objects.walk(visit);
                filter.walk(visit);
            }
            Condition::EnteredThisTurn(objects)
            | Condition::Token(objects)
            | Condition::OnBattlefield(objects)
            | Condition::Cast(objects)
            | Condition::HasChosenName(objects) => {
                objects.walk(visit);
            }
            Condition::MeldPair(partner) => partner.walk(visit),
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
    /// The subtypes named in its modifications and text changes.
    #[allow(clippy::too_many_lines)]
    fn subtypes(&self) -> Subtypes {
        let found = std::cell::RefCell::new(Subtypes::NONE);
        self.any(&|instruction| {
            let mut found = found.borrow_mut();
            match instruction {
                Effect::Modify { modifications, .. } => {
                    for given in modifications
                        .iter()
                        .filter_map(Modification::granted_subtypes)
                    {
                        *found = found.union(given);
                    }
                }
                Effect::ChangeText { words, .. } if words.land_types() => {
                    *found = (BasicLandTypes::ALL.subtypes()).fold(*found, Subtypes::with);
                }
                // A change of color words alone makes no subtype word read otherwise.
                Effect::ChangeText { .. }
                | Effect::Sequence(_)
                | Effect::If { .. }
                | Effect::May { .. }
                | Effect::Unless { .. }
                | Effect::ForEachPlayer { .. }
                | Effect::Choose { .. }
                | Effect::ChooseAtRandom(_)
                | Effect::LookAt { .. }
                | Effect::ChooseValue { .. }
                | Effect::Counter(_)
                | Effect::CounterInto { .. }
                | Effect::CopySpell { .. }
                | Effect::Destroy { .. }
                | Effect::Regenerate(_)
                | Effect::Tap(_)
                | Effect::Untap(_)
                | Effect::PhaseOut(_)
                | Effect::SetPrepared { .. }
                | Effect::Exile(_)
                | Effect::ExileUntilThisLeaves(_)
                | Effect::ExileLinked(_)
                | Effect::Sacrifice(_)
                | Effect::ReturnToHand(_)
                | Effect::PutIntoGraveyard(_)
                | Effect::ReturnToBattlefield(_)
                | Effect::PutOntoBattlefield(_)
                | Effect::ReturnTransformed(_)
                | Effect::Transform(_)
                | Effect::Meld(_)
                | Effect::Flip(_)
                | Effect::Reveal(_)
                | Effect::PutIntoLibrary { .. }
                | Effect::PutBack { .. }
                | Effect::PutAnyOnBottom { .. }
                | Effect::ShuffleInto { .. }
                | Effect::Dig { .. }
                | Effect::ExileUntil { .. }
                | Effect::Fateseal { .. }
                | Effect::Wish { .. }
                | Effect::Search { .. }
                | Effect::Draw { .. }
                | Effect::Discard { .. }
                | Effect::Mill { .. }
                | Effect::Scry { .. }
                | Effect::Explore(_)
                | Effect::ManifestDread(_)
                | Effect::Subgame
                | Effect::FlipCoin(_)
                | Effect::DealDamage { .. }
                | Effect::PreventDamage { .. }
                | Effect::GainLife { .. }
                | Effect::LoseLife { .. }
                | Effect::CreateTokens { .. }
                | Effect::CreateAttackingTokens { .. }
                | Effect::CreateCopies { .. }
                | Effect::BecomeCopy { .. }
                | Effect::Mutate(_)
                | Effect::AddCounters { .. }
                | Effect::RemoveCounters { .. }
                | Effect::RemoveAllCounters(_)
                | Effect::AddMana { .. }
                | Effect::AddManaOfKind { .. }
                | Effect::AddProduct { .. }
                | Effect::ExchangeControl(..)
                | Effect::Attach { .. }
                | Effect::ChangeRule { .. }
                | Effect::ChangeMana { .. }
                | Effect::GainFlashback(_)
                | Effect::PermitCast { .. }
                | Effect::MayCast { .. }
                | Effect::UntapUpTo { .. }
                | Effect::Delay { .. }
                | Effect::GetEmblem { .. }
                | Effect::WinGame(_)
                | Effect::LoseGame(_)
                | Effect::ControlPlayer(_)
                | Effect::RestartGame { .. }
                | Effect::EndTurn
                | Effect::AdditionalStep(_)
                | Effect::AdditionalPhases { .. }
                | Effect::ExtraTurn(..) => {}
            }
            false
        });
        found.into_inner()
    }

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
        match self.branches() {
            Branches::Sequence(effects) => {
                for effect in effects {
                    effect.walk(visit);
                }
            }
            Branches::If {
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
            Branches::May(effect)
            | Branches::Unless(effect, _)
            | Branches::ForEachPlayer(effect)
            | Branches::WhenFound(effect) => {
                effect.walk(visit);
            }
            Branches::Single => {
                for modification in self.modifications() {
                    modification.walk(visit);
                }
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
