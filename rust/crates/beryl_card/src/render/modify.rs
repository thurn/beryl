//! The template of a resolving spell's or ability's continuous effect ([`Effect::Modify`]):
//! "it gains first strike", "gain control of target artifact", "target artifact or creature
//! loses all abilities and becomes a green Elk creature with base power and toughness 3/3".
//!
//! [`Effect::Modify`]: crate::Effect::Modify

use super::Ctx;
use super::abilities::{a_types, possessive};
use super::words::{colors, keywords, list, pt_change, types, with_article};
use crate::continuous::Modification;
use crate::effect::{Objects, Players};

/// One part of the effect in words: a whole clause, or what the objects do, said after them.
enum Said {
    Clause(String),
    Predicate(String),
}

impl Ctx {
    /// What a resolving spell's continuous effect does, its parts in one sentence with its
    /// subject once ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)).
    pub(super) fn modify(&mut self, objects: Objects, modifications: &[Modification]) -> String {
        let plural = self.is_plural(objects);
        let who = self.objects(objects);
        let becoming = becomes_one_thing(modifications);
        let mut clauses = Vec::new();
        let mut predicates = Vec::new();
        let mut said_becoming = false;
        for &modification in modifications {
            if becoming && part_of_becoming(modification) {
                if !said_becoming {
                    predicates.push(self.becomes(modifications, plural));
                    said_becoming = true;
                }
                continue;
            }
            match self.part(objects, modification, &who, plural) {
                Said::Clause(clause) => clauses.push(clause),
                Said::Predicate(predicate) => predicates.push(predicate),
            }
        }
        if !predicates.is_empty() {
            clauses.insert(0, format!("{who} {}", list(&predicates, "and")));
        }
        self.plural = plural;
        list(&clauses, "and")
    }

    /// "Becomes a green Elk creature with base power and toughness 3/3": the type, color,
    /// subtype and base power and toughness parts of one effect said together, as a card
    /// says them ([CR 205.1a](https://yawgatog.com/resources/magic-rules/#R2051a)): "the new
    /// card type(s) replaces any existing card types".
    fn becomes(&mut self, modifications: &[Modification], plural: bool) -> String {
        let mut words = Vec::new();
        let mut base = None;
        for &modification in modifications {
            match modification {
                Modification::SetColors(set) => words.push(list(&colors(set), "and")),
                Modification::SetSubtype(subtype) => words.push(subtype.name().to_owned()),
                Modification::SetBasePt(power, toughness) => base = Some((*power, *toughness)),
                _ => {}
            }
        }
        for &modification in modifications {
            if let Modification::SetTypes(set) = modification {
                words.push(types(set).join(" "));
            }
        }
        let verb = if plural { "become" } else { "becomes" };
        let mut text = format!("{verb} {}", with_article(&words.join(" ")));
        if let Some((power, toughness)) = base {
            text = format!("{text} with {}", self.base_pt(power, toughness));
        }
        text
    }

    /// One part of the effect, said of `who`.
    fn part(
        &mut self,
        objects: Objects,
        modification: Modification,
        who: &str,
        plural: bool,
    ) -> Said {
        let verb =
            |single: &'static str, several: &'static str| if plural { several } else { single };
        Said::Predicate(match modification {
            Modification::Control => {
                return Said::Clause(self.clause(Players::You, ("gain", "gains"), |c| {
                    format!("control of {}", c.objects(objects))
                }));
            }
            Modification::SetPower(amount) => {
                return Said::Clause(format!(
                    "{} power becomes {}",
                    possessive(who),
                    self.amount(*amount)
                ));
            }
            Modification::SwitchPt => {
                return Said::Clause(format!("switch {} power and toughness", possessive(who)));
            }
            Modification::AddTypes(set) => {
                format!("{} {}", verb("becomes", "become"), a_types(set))
            }
            Modification::RemoveTypes(set) => {
                format!("{} {}", verb("stops being", "stop being"), a_types(set))
            }
            Modification::SetTypes(set) => format!(
                "{} {} and {} all other card types",
                verb("becomes", "become"),
                a_types(set),
                verb("loses", "lose")
            ),
            Modification::AddSubtype(subtype) => format!(
                "{} {} in addition to its other types",
                verb("becomes", "become"),
                with_article(subtype.name())
            ),
            Modification::SetSubtype(subtype) => {
                format!(
                    "{} {}",
                    verb("becomes", "become"),
                    with_article(subtype.name())
                )
            }
            Modification::AddKeywords(set) => {
                format!("{} {}", verb("gains", "gain"), list(&keywords(set), "and"))
            }
            Modification::Grant(ix) => format!("{} {}", verb("gains", "gain"), self.granted(ix)),
            Modification::ModifyPt(power, toughness) => {
                format!("{} {}", verb("gets", "get"), pt_change(power, toughness))
            }
            Modification::LoseAllAbilities => format!("{} all abilities", verb("loses", "lose")),
            Modification::LoseAbility(ix) => {
                format!("{} {}", verb("loses", "lose"), self.own_ability(ix))
            }
            Modification::SetColors(set) => {
                format!(
                    "{} {}",
                    verb("becomes", "become"),
                    list(&colors(set), "and")
                )
            }
            Modification::AddColors(set) => format!(
                "{} {} in addition to its other colors",
                verb("becomes", "become"),
                list(&colors(set), "and")
            ),
            Modification::SetBasePt(power, toughness) => {
                format!(
                    "{} {}",
                    verb("has", "have"),
                    self.base_pt(*power, *toughness)
                )
            }
        })
    }
}

/// Whether the effect sets the card types and something more that a card says in the same
/// words, "becomes a green Elk creature with base power and toughness 3/3".
fn becomes_one_thing(modifications: &[Modification]) -> bool {
    modifications
        .iter()
        .any(|m| matches!(m, Modification::SetTypes(_)))
        && modifications
            .iter()
            .filter(|&&m| part_of_becoming(m))
            .count()
            > 1
}

/// Whether a part is said in "becomes a [colors] [subtype] [types] with base power and
/// toughness [P/T]".
fn part_of_becoming(modification: Modification) -> bool {
    matches!(
        modification,
        Modification::SetTypes(_)
            | Modification::SetColors(_)
            | Modification::SetSubtype(_)
            | Modification::SetBasePt(..)
    )
}
