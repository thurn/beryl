//! The rule changes a game's cards can make, numbered (the crate's contract, "Static rule
//! changes").
//!
//! [CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311): "Some continuous effects affect
//! game rules rather than objects." A card says one as data (`beryl_card`'s [`RuleChange`]), and
//! any number of cards may say any number of them, so a game numbers those its own cards name
//! ([`RuleId`]) as its table is built. A set of them is then one word ([`Rules`]), an entry of the
//! Effects store holds one in a byte, and the table keeps the set of each kind the rules check
//! ([`RuleKind`]): whether a change of a kind is in force for a player is one AND, and only when
//! one is does a check read what it says.

use beryl_card::{
    Ability, AbilityKind, CardDef, Condition, Face, Filter, Governed, Objects, Players, RuleChange,
    When,
};

use crate::event::Listens;

/// Why the rules cannot check a rule change: the part of it no code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unchecked {
    /// It governs what no check reads: the loyalty abilities of permanents other than the
    /// one with the ability, or their timing as the rules have it already; or an effect
    /// starts one that is asked of a permanent.
    Rule,
    /// It governs casting the spells a filter accepts, and the filter asks for more than a
    /// spell's own characteristics.
    Filter,
    /// It is read for players other than "you", "each opponent" and "each player", or, of
    /// a permanent's own loyalty abilities, for any but "you".
    Players,
    /// A static ability changes a rule for players only as long as a condition holds: the
    /// players it applies to are found with the layers, which read no condition. Or two
    /// abilities make one rule change on different conditions.
    Condition,
    /// The game's cards name more rule changes than a set of them holds
    /// ([`Rules::CAPACITY`]).
    Rules,
}

/// The number of a rule change in a game: its place among those the game's cards name, in
/// order of first appearance ([`crate::GameTable::rule`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleId(u8);

impl RuleId {
    #[inline]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// A set of a game's rule changes, a bit for each [`RuleId`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rules(u64);

impl Rules {
    /// The rule changes a game can number: a game whose cards name more has no table
    /// ([`Unchecked::Rules`]).
    pub const CAPACITY: usize = u64::BITS as usize;

    pub const NONE: Rules = Rules(0);

    /// The set with `rule` in it too.
    #[inline]
    #[must_use]
    pub const fn with(self, rule: RuleId) -> Rules {
        Rules(self.0 | 1 << rule.0)
    }

    #[inline]
    #[must_use]
    pub const fn union(self, other: Rules) -> Rules {
        Rules(self.0 | other.0)
    }

    /// The rule changes in both sets.
    #[inline]
    #[must_use]
    pub const fn common(self, other: Rules) -> Rules {
        Rules(self.0 & other.0)
    }

    /// The rule changes of this set that are not in `other`.
    #[inline]
    #[must_use]
    pub const fn without(self, other: Rules) -> Rules {
        Rules(self.0 & !other.0)
    }

    #[inline]
    pub const fn contains(self, rule: RuleId) -> bool {
        self.0 & (1 << rule.0) != 0
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Its rule changes, in the order the game numbered them.
    #[inline]
    pub fn iter(self) -> impl Iterator<Item = RuleId> {
        let mut left = self.0;
        std::iter::from_fn(move || {
            (left != 0).then(|| {
                let id = left.trailing_zeros() as u8;
                left &= left - 1;
                RuleId(id)
            })
        })
    }
}

/// What the rules check a rule change at: the kinds of change the engine has code for,
/// each read where the rule it changes is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleKind {
    /// Some players may cast some spells only when they could cast a sorcery ([CR 307.1]).
    ///
    /// [CR 307.1]: https://yawgatog.com/resources/magic-rules/#R3071
    CastOnlyAsSorcery,
    /// Some players may cast some spells any time they could cast an instant, as though the spells
    /// had flash ([CR 702.8a]).
    ///
    /// [CR 702.8a]: https://yawgatog.com/resources/magic-rules/#R7028a
    CastAsInstant,
    /// The loyalty abilities of the permanent with the ability may be activated any time its
    /// controller could cast an instant, where [CR 606.3] has "during a main phase of their turn".
    ///
    /// [CR 606.3]: https://yawgatog.com/resources/magic-rules/#R6063
    LoyaltyAsInstant,
}

impl RuleKind {
    const COUNT: usize = 3;

    /// The kind of change `rule` is, or the part of it no code reads.
    pub fn of(rule: &RuleChange) -> Result<RuleKind, Unchecked> {
        match (rule.what, rule.when) {
            (Governed::CastSpells(filter), when) => {
                if !of_a_spells_characteristics(filter) {
                    return Err(Unchecked::Filter);
                }
                if !matches!(
                    rule.players,
                    Players::You | Players::EachOpponent | Players::EachPlayer
                ) {
                    return Err(Unchecked::Players);
                }
                Ok(match when {
                    When::OnlyAsSorcery => RuleKind::CastOnlyAsSorcery,
                    When::AsInstant => RuleKind::CastAsInstant,
                })
            }
            // Only its controller activates a permanent's abilities (CR 606.3), so the
            // change is asked of the permanent, and is for "you" or for nobody.
            (Governed::ActivateLoyaltyAbilities(Objects::This), When::AsInstant) => {
                if matches!(rule.players, Players::You) {
                    Ok(RuleKind::LoyaltyAsInstant)
                } else {
                    Err(Unchecked::Players)
                }
            }
            // Other permanents' loyalty abilities, and a restriction to the timing the
            // rules have already.
            (Governed::ActivateLoyaltyAbilities(_), _) => Err(Unchecked::Rule),
        }
    }

    /// Whether a change of this kind applies to players, who are found with the layers; a
    /// change of any other kind is asked of the permanent with the ability.
    #[inline]
    pub const fn of_players(self) -> bool {
        matches!(self, RuleKind::CastOnlyAsSorcery | RuleKind::CastAsInstant)
    }
}

/// Whether a filter asks only for what a spell is by its own characteristics, which is all
/// that is read of a spell a player has yet to cast.
fn of_a_spells_characteristics(filter: Filter) -> bool {
    match filter {
        Filter::Any
        | Filter::Types(_)
        | Filter::Supertypes(_)
        | Filter::Subtype(_)
        | Filter::Colors(_) => true,
        Filter::Whose(_)
        | Filter::Tapped
        | Filter::ManaValueAtMost(_)
        | Filter::ManaValueAtMostX => false,
        Filter::Not(filter) => of_a_spells_characteristics(*filter),
        Filter::All(filters) | Filter::AnyOf(filters) => filters
            .iter()
            .all(|&filter| of_a_spells_characteristics(filter)),
    }
}

/// The rule changes the cards of one game name, numbered, and the set of each kind.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuleChanges {
    list: Vec<&'static RuleChange>,
    of_kind: [Rules; RuleKind::COUNT],
    /// Those that govern casting every spell, whatever it is.
    of_every_spell: Rules,
    /// For each, the condition of the static ability that makes it for its own permanent,
    /// if it has one ([`RuleChanges::keep_condition`]).
    conditions: Vec<Option<&'static Condition>>,
}

impl RuleChanges {
    /// The number of `rule`, a new one if no card before named it, or why the engine
    /// cannot check it. A rule change is the one written at its address: two cards that
    /// say the same words name two.
    pub(crate) fn number(&mut self, rule: &'static RuleChange) -> Result<RuleId, Unchecked> {
        if let Some(id) = self.id(rule) {
            return Ok(id);
        }
        let kind = RuleKind::of(rule)?;
        if self.list.len() == Rules::CAPACITY {
            return Err(Unchecked::Rules);
        }
        let id = RuleId(u8::try_from(self.list.len()).expect("fewer than 64 rule changes"));
        self.list.push(rule);
        self.conditions.push(None);
        self.of_kind[kind as usize] = self.of_kind[kind as usize].with(id);
        if matches!(rule.what, Governed::CastSpells(Filter::Any)) {
            self.of_every_spell = self.of_every_spell.with(id);
        }
        Ok(id)
    }

    /// Keeps `condition` as the one the static ability that makes `rule` for its own
    /// permanent has, so that a check reads it by the rule change's number and walks no
    /// ability. A rule change is made on one condition: two abilities that name one with
    /// different conditions are refused.
    pub(crate) fn keep_condition(
        &mut self,
        id: RuleId,
        condition: Option<&'static Condition>,
        first: bool,
    ) -> Result<(), Unchecked> {
        let kept = &mut self.conditions[id.index()];
        let same = match (*kept, condition) {
            (None, None) => true,
            (Some(kept), Some(condition)) => std::ptr::eq(kept, condition),
            _ => false,
        };
        if !first && !same {
            return Err(Unchecked::Condition);
        }
        *kept = condition;
        Ok(())
    }

    /// The condition of the static ability that makes the rule change `id` for its own
    /// permanent, if it has one.
    #[inline]
    pub(crate) fn condition(&self, id: RuleId) -> Option<&'static Condition> {
        self.conditions[id.index()]
    }

    /// The number of `rule`, if a card of the game names it.
    #[inline]
    pub(crate) fn id(&self, rule: &'static RuleChange) -> Option<RuleId> {
        let at = self.list.iter().position(|&r| std::ptr::eq(r, rule))?;
        Some(RuleId(
            u8::try_from(at).expect("fewer than 64 rule changes"),
        ))
    }

    /// For each of `defs`, the rule changes the static abilities of its first two faces
    /// make, and those of any of its faces.
    pub(crate) fn of_cards(&self, defs: &[&'static CardDef]) -> (Vec<[Rules; 2]>, Vec<Rules>) {
        let of = |def: &CardDef, i: usize| {
            (def.faces.get(i)).map_or(Rules::NONE, |face| self.of_face(face))
        };
        let faces = defs.iter().map(|def| [of(def, 0), of(def, 1)]).collect();
        let any = defs
            .iter()
            .map(|def| (def.faces.iter()).fold(Rules::NONE, |r, face| r.union(self.of_face(face))))
            .collect();
        (faces, any)
    }

    /// The rule changes the static abilities of `face` make, by their numbers.
    pub(crate) fn of_face(&self, face: &Face) -> Rules {
        (face.abilities.iter())
            .filter(|ability| ability.kind() == AbilityKind::Static)
            .filter_map(|ability| self.id(ability.rule()?))
            .fold(Rules::NONE, Rules::with)
    }

    #[inline]
    pub(crate) fn get(&self, id: RuleId) -> &'static RuleChange {
        self.list[id.index()]
    }

    #[inline]
    pub(crate) fn of_kind(&self, kind: RuleKind) -> Rules {
        self.of_kind[kind as usize]
    }

    #[inline]
    pub(crate) fn of_every_spell(&self) -> Rules {
        self.of_every_spell
    }

    /// The rule changes that apply to players ([`RuleKind::of_players`]).
    #[inline]
    pub(crate) fn of_players(&self) -> Rules {
        self.of_kind(RuleKind::CastOnlyAsSorcery)
            .union(self.of_kind(RuleKind::CastAsInstant))
    }
}

/// Numbers the rule changes the abilities of a card's faces name, or of the abilities they
/// grant, and gives [`Listens::RULE_CHANGE`] if a static ability of it makes one for players:
/// the bit of its listener mask no event has. Or the first of them the rules cannot check.
pub(crate) fn number(def: &CardDef, rules: &mut RuleChanges) -> Result<Listens, Unchecked> {
    let mut mask = Listens::NONE;
    for face in def.faces {
        for ability in face.abilities.iter().chain(face.grants) {
            mask |= named_by(ability, rules)?;
        }
    }
    Ok(mask)
}

/// Numbers the rule change `ability` names, if it names one, and gives
/// [`Listens::RULE_CHANGE`] if it is a static ability's for players.
fn named_by(ability: &Ability, rules: &mut RuleChanges) -> Result<Listens, Unchecked> {
    let Some(rule) = ability.rule() else {
        return Ok(Listens::NONE);
    };
    let first = rules.id(rule).is_none();
    let id = rules.number(rule)?;
    let of_players = RuleKind::of(rule)?.of_players();
    match (ability.kind() == AbilityKind::Static, of_players) {
        (true, true) if ability.condition().is_some() => Err(Unchecked::Condition),
        (true, true) => Ok(Listens::RULE_CHANGE),
        // Asked of the permanent, with its condition, as the rule is applied.
        (true, false) => {
            rules.keep_condition(id, ability.condition(), first)?;
            Ok(Listens::NONE)
        }
        // Started by the ability's effect, on the players it names as it resolves.
        (false, true) => Ok(Listens::NONE),
        (false, false) => Err(Unchecked::Rule),
    }
}

/// Three rule changes and a card that names them, for the crate's tests.
#[cfg(test)]
pub(crate) mod fixtures {
    use beryl_card::{
        Ability, CardDef, Condition, Face, Filter, Governed, Objects, Players, RuleChange, Types,
        When,
    };

    use crate::table::{CardTable, GameTable};

    /// "You may cast sorcery spells as though they had flash."
    pub(crate) static FLASH: RuleChange = RuleChange {
        players: Players::You,
        what: Governed::CastSpells(Filter::Types(Types::SORCERY)),
        when: When::AsInstant,
    };

    /// "Each opponent can cast spells only any time they could cast a sorcery."
    pub(crate) static LIMIT: RuleChange = RuleChange {
        players: Players::EachOpponent,
        what: Governed::CastSpells(Filter::Any),
        when: When::OnlyAsSorcery,
    };

    /// "You may activate this permanent's loyalty abilities any time you could cast an
    /// instant."
    pub(crate) static LOYALTY: RuleChange = RuleChange {
        players: Players::You,
        what: Governed::ActivateLoyaltyAbilities(Objects::This),
        when: When::AsInstant,
    };

    /// A planeswalker whose static abilities make [`LIMIT`] and, as long as it entered this
    /// turn, [`LOYALTY`], and a loyalty ability of it whose effect starts [`FLASH`].
    pub(crate) static WALKER: CardDef = CardDef {
        name: "Walker",
        faces: &[Face {
            types: Types::PLANESWALKER,
            loyalty: Some(3),
            abilities: &[
                Ability::rule_change(&LOYALTY).only_if(&Condition::EnteredThisTurn(Objects::This)),
                Ability::rule_change(&LIMIT),
                Ability::loyalty(1).starting(&FLASH),
            ],
            ..Face::EMPTY
        }],
        tokens: &[],
    };

    pub(crate) static WALKERS: CardTable = CardTable::new(&[&WALKER]);

    /// The table of a game with a [`WALKER`] in it.
    pub(crate) fn table() -> GameTable {
        GameTable::new(&WALKERS, [&[(1, "Walker")], &[]]).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{FLASH, LIMIT, LOYALTY, table};
    use super::*;

    #[test]
    fn a_game_numbers_the_rule_changes_its_cards_name_once_each() {
        let t = table();
        // In order of first appearance, whichever kind of ability names one.
        let [loyalty, limit, flash] =
            [&LOYALTY, &LIMIT, &FLASH].map(|rule| t.rule_id(rule).unwrap());
        assert_eq!([loyalty.index(), limit.index(), flash.index()], [0, 1, 2]);
        assert_eq!(t.rule_id(t.rule(limit)), Some(limit));
        assert_eq!(
            t.rules_of(RuleKind::CastOnlyAsSorcery),
            Rules::NONE.with(limit)
        );
        assert_eq!(t.rules_of(RuleKind::CastAsInstant), Rules::NONE.with(flash));
        assert_eq!(
            t.rules_of(RuleKind::LoyaltyAsInstant),
            Rules::NONE.with(loyalty)
        );
        assert_eq!(t.player_rules(), Rules::NONE.with(limit).with(flash));
        let all = Rules::NONE.with(loyalty).with(limit).with(flash);
        assert_eq!(all.iter().collect::<Vec<_>>(), [loyalty, limit, flash]);
        assert!(all.without(t.player_rules()).contains(loyalty));
        assert!(Rules::NONE.is_empty() && Rules::NONE.common(all).is_empty());
    }
}
