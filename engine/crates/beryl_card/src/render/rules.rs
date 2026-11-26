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
                    When::OnlyAsSorcery | When::AsInstant | When::AsUsual => {
                        format!("{creatures} can block only at a changed time")
                    }
                };
            }
            Governed::UntapInUntapStep(objects) => {
                return format!(
                    "{} doesn't untap during your untap step",
                    self.objects(objects)
                );
            }
            Governed::BlockAnyNumber(objects) => {
                return format!(
                    "{} can block any number of creatures",
                    self.objects(objects)
                );
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
                    (When::OnlyAsSorcery | When::AsInstant | When::AsUsual, _) => {
                        format!("{sources} can be activated only at a changed time")
                    }
                };
            }
            // "This spell can't be countered": of the spells, not of who would counter them.
            Governed::Counter(objects) => {
                let spells = self.objects(objects);
                return match rule.when {
                    When::Never => format!("{spells} can't be countered"),
                    When::OnlyAsSorcery | When::AsInstant | When::AsUsual => {
                        format!("{spells} can be countered only at a changed time")
                    }
                };
            }
            Governed::PreventDamage => return "damage can't be prevented".to_owned(),
            Governed::TakeStep(step) => {
                let step = super::words::step_noun(step);
                if rule.players == Players::EachPlayer {
                    return format!("players skip their {step}s");
                }
                let whose = self.possessive(rule.players);
                let verb = if rule.players == Players::You {
                    "skip"
                } else {
                    "skips"
                };
                return format!("{who} {verb} {whose} {step}s");
            }
            Governed::Player(changes) => return self.player_rules(rule.players, changes),
            Governed::SpellCosts { spells, change } => {
                return self.spell_cost_change(spells, change, rule.players, &who);
            }
            Governed::SpendAsAnyColor(filter) => {
                let permanents = self.noun(filter, Kind::Permanent).plural(Kind::Permanent);
                return format!(
                    "{who} may spend mana as though it were mana of any color to activate abilities of {permanents}"
                );
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

    /// Renders a cost change with the affected spells as the subject.
    fn spell_cost_change(
        &mut self,
        spells: Filter,
        change: SpellCost,
        players: Players,
        who: &str,
    ) -> String {
        let cast = match players {
            Players::EachPlayer => String::new(),
            Players::You => " you cast".to_owned(),
            Players::EachOpponent => " your opponents cast".to_owned(),
            Players::Target(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::Enchanted
            | Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::EachOther(_) => format!(" {who} casts"),
        };
        let noun = self.noun(spells, Kind::Spell);
        match change {
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
        if changes.contains(PlayerRules::NO_MAXIMUM_HAND_SIZE) {
            clauses.push(self.stated(players, ("have", "has"), |_| {
                "no maximum hand size".to_owned()
            }));
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

    /// Renders a change to a permanent's prepared designation.
    pub(super) fn set_prepared(&mut self, objects: Objects, prepared: bool) -> String {
        let verb = if self.is_plural(objects) {
            "become"
        } else {
            "becomes"
        };
        let designation = if prepared { "prepared" } else { "unprepared" };
        format!("{} {verb} {designation}", self.objects(objects))
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
            When::OnlyAsSorcery | When::AsInstant | When::AsUsual => {
                format!("{creatures} can attack only at a changed time")
            }
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
