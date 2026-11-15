//! The templates of a change to a rule of the game ([`RuleChange`]) and of a restriction on
//! the mana a mana ability makes ([`ManaRestriction`]).

use super::Ctx;
use super::filters::Kind;
use super::words::number;
use crate::continuous::{Governed, Named, PlayerRules, RuleChange, SpellCost, When};
use crate::effect::{Objects, Players};
use crate::filter::Filter;
use crate::mana::ManaRestriction;

impl Ctx {
    /// A change to a rule of the game, such as "each opponent can cast spells only any time
    /// they could cast a sorcery" or "you may cast sorcery spells as though they had flash"
    /// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)).
    pub(super) fn rule_change(&mut self, rule: &RuleChange) -> String {
        let who = self.player(rule.players);
        let they = if rule.players == Players::You {
            "you"
        } else {
            "they"
        };
        let what = match rule.what {
            Governed::CastSpells(filter) => {
                let spells = self.noun(filter, Kind::Spell).plural(Kind::Spell);
                format!("cast {spells}")
            }
            Governed::ActivateLoyaltyAbilities(objects) => {
                let whose = self.objects_possessive(objects);
                format!("activate {whose} loyalty abilities")
            }
            // "This creature can't block": of the creatures, not of who declares blocks.
            Governed::Block(objects) => {
                let creatures = self.objects(objects);
                return match rule.when {
                    When::Never => format!("{creatures} can't block"),
                    _ => format!("{creatures} can block only at a changed time"),
                };
            }
            // "This creature can't attack unless defending player controls an Island": of the
            // creatures, not of who declares attacks.
            Governed::Attack {
                attackers,
                unless_defender_controls,
            } => return self.attack_restriction(attackers, unless_defender_controls, rule.when),
            // Of the abilities, not of who would activate them.
            Governed::ActivateAbilities {
                sources: Named::Chosen,
                mana_abilities,
            } => {
                let sources = "activated abilities of sources with the chosen name";
                return match (rule.when, mana_abilities) {
                    (When::Never, false) => {
                        format!("{sources} can't be activated unless they're mana abilities")
                    }
                    (When::Never, true) => format!("{sources} can't be activated"),
                    _ => format!("{sources} can be activated only at a changed time"),
                };
            }
            // "This spell can't be countered": of the spells, not of who would counter them.
            Governed::Counter(objects) => {
                let spells = self.objects(objects);
                return match rule.when {
                    When::Never => format!("{spells} can't be countered"),
                    _ => format!("{spells} can be countered only at a changed time"),
                };
            }
            Governed::Player(changes) => return self.player_rules(rule.players, changes),
            // Of the spells, not of who casts them: "Noncreature spells cost {1} more to
            // cast."
            Governed::SpellCosts { spells, change } => {
                let cast = match rule.players {
                    Players::EachPlayer => String::new(),
                    Players::You => " you cast".to_owned(),
                    Players::EachOpponent => " your opponents cast".to_owned(),
                    _ => format!(" {who} casts"),
                };
                let noun = self.noun(spells, Kind::Spell);
                return match change {
                    SpellCost::More(n) => {
                        format!(
                            "{}{cast} cost {{{n}}} more to cast",
                            noun.plural(Kind::Spell)
                        )
                    }
                    SpellCost::Less(n) => {
                        format!(
                            "{}{cast} cost {{{n}}} less to cast",
                            noun.plural(Kind::Spell)
                        )
                    }
                    SpellCost::AtLeast(n) => {
                        let n = number(n);
                        format!(
                            "each {}{cast} that would cost less than {n} mana to cast costs \
                             {n} mana to cast",
                            noun.singular(Kind::Spell)
                        )
                    }
                };
            }
        };
        match (rule.when, rule.what) {
            (When::Never, _) => format!("{who} can't {what}"),
            (When::OnlyAsSorcery, _) => {
                format!("{who} can {what} only any time {they} could cast a sorcery")
            }
            (When::AsInstant, Governed::ActivateLoyaltyAbilities(_)) => {
                format!("{who} may {what} any time {they} could cast an instant")
            }
            (When::AsInstant, _) => format!("{who} may {what} as though they had flash"),
            (When::AsUsual, _) => format!("{who} {what} as usual"),
        }
    }

    /// A change to players themselves: "your life total can't change and you gain protection
    /// from everything" ([CR 119.8], [CR 702.16j]).
    ///
    /// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
    /// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
    fn player_rules(&mut self, players: Players, changes: PlayerRules) -> String {
        let mut clauses = Vec::new();
        if changes.contains(PlayerRules::LIFE_CANT_CHANGE) {
            let whose = self.possessive(players);
            clauses.push(format!("{whose} life total can't change"));
        }
        if changes.contains(PlayerRules::PROTECTION_FROM_EVERYTHING) {
            let who = self.player(players);
            let gain = if players == Players::You {
                "gain"
            } else {
                "gains"
            };
            clauses.push(format!("{who} {gain} protection from everything"));
        }
        clauses.join(" and ")
    }

    /// "All permanents you control phase out" ([CR 702.26b]): the permanents are the subject.
    ///
    /// [CR 702.26b]: https://yawgatog.com/resources/magic-rules/#R70226b
    pub(super) fn phase_out(&mut self, objects: Objects) -> String {
        let phase = if self.is_plural(objects) {
            "phase"
        } else {
            "phases"
        };
        format!("{} {phase} out", self.objects(objects))
    }

    /// "This creature can't attack unless defending player controls an Island"
    /// ([CR 508.1c](https://yawgatog.com/resources/magic-rules/#R5081c)).
    fn attack_restriction(
        &mut self,
        attackers: Objects,
        unless_defender_controls: Option<Filter>,
        when: When,
    ) -> String {
        let creatures = self.objects(attackers);
        let unless = unless_defender_controls.map_or(String::new(), |filter| {
            let permanent = self
                .noun(filter, Kind::Permanent)
                .indefinite(Kind::Permanent);
            format!(" unless defending player controls {permanent}")
        });
        match when {
            When::Never => format!("{creatures} can't attack{unless}"),
            _ => format!("{creatures} can attack only at a changed time"),
        }
    }

    /// "Spend this mana only to cast a creature spell of the chosen type, and that spell
    /// can't be countered." ([CR 106.6]).
    ///
    /// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
    pub(super) fn spend_only(&mut self, restriction: &ManaRestriction) -> String {
        let spell = self
            .noun(restriction.cast, Kind::Spell)
            .indefinite(Kind::Spell);
        let chosen = if restriction.of_chosen_type {
            " of the chosen type"
        } else {
            ""
        };
        let rider = match restriction.rider {
            Some(&RuleChange {
                what: Governed::Counter(_),
                when: When::Never,
                ..
            }) => ", and that spell can't be countered",
            Some(_) => ", and that spell is changed",
            None => "",
        };
        format!("Spend this mana only to cast {spell}{chosen}{rider}.")
    }
}
