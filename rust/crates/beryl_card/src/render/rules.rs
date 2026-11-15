//! The templates of a change to a rule of the game ([`RuleChange`]) and of a restriction on
//! the mana a mana ability makes ([`ManaRestriction`]).

use super::Ctx;
use super::filters::Kind;
use crate::continuous::{Governed, Named, RuleChange, When};
use crate::effect::Players;
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
