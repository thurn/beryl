//! The templates of an ability's parts: its cost ([`Cost`]), its trigger ([`Trigger`]),
//! what a static ability does to objects ([`LayerSpec`]), a replacement ([`Replacement`]),
//! and the whole ability as one paragraph. A rule change's template is in `rules.rs`.

use super::Ctx;
use super::designations::roman;
use super::filters::Kind;
use super::words::{
    colors, keywords, list, mana_text, protection, pt_change, sentence, types, with_article,
};
use crate::ability::{Ability, AbilityKind, Behavior, Timing};
use crate::continuous::{LayerSpec, Modification, Replacement, Rewrite};
use crate::cost::{CastingKind, Cost, CostPart, ESCAPE_EXILES, Measure};
use crate::effect::{Amount, Condition, Effect, Objects, Players};
use crate::ids::{Zone, ZoneSet};
use crate::index::GrantIx;
use crate::trigger::{Subject, TriggerEvent};

impl Ctx {
    /// One ability as its paragraph of rules text, each clause a sentence.
    pub(super) fn paragraph(&mut self, ability: &Ability) -> String {
        // The text of an ability that works only in a graveyard (CR 113.6b) calls its card
        // this card, as in "return this card from your graveyard to the battlefield".
        let away = ability.zones() == ZoneSet::GRAVEYARD && !self.in_graveyard;
        let this = away.then(|| std::mem::replace(&mut self.this, "this card".to_owned()));
        self.in_graveyard |= away;
        let mut text = self.paragraph_here(ability);
        match ability.designation() {
            crate::Designation::Solved => text = format!("Solved — {text}"),
            crate::Designation::Imprint => text = format!("Imprint — {text}"),
            crate::Designation::None
            | crate::Designation::LevelUp(_)
            | crate::Designation::AtLevel(_)
            | crate::Designation::ToSolve => {}
        }
        if let Some(this) = this {
            (self.this, self.in_graveyard) = (this, false);
        }
        text
    }

    fn paragraph_here(&mut self, ability: &Ability) -> String {
        self.begin(ability.targets(), ability.trigger());
        self.ability = Some(*ability);
        if let Some(text) = self.designation_paragraph(ability) {
            return text;
        }
        if let Some(text) = self.level_symbol_paragraph(ability) {
            return text;
        }
        if let Some(keyword) = Ctx::keyword_line(ability) {
            return keyword;
        }
        if let Some(condition) = ability.companion_condition() {
            return format!("Companion — {}", sentence(&self.deck_condition(condition)));
        }
        if let Some((first, last)) = ability.chapter_numbers() {
            let chapters = (first..=last).map(roman).collect::<Vec<_>>().join(", ");
            return format!("{chapters} — {}", sentences(&self.body(ability.body())));
        }
        let clauses = match ability.kind() {
            AbilityKind::Spell => {
                if let Some(enchant) = self.enchant(ability) {
                    return enchant;
                }
                self.body(ability.body())
            }
            AbilityKind::Activated => {
                let cost = self.cost_text(ability.cost());
                // A cost that sacrifices the object names it, so its effect says "it":
                // "Sacrifice this creature: It deals 1 damage to any target."
                self.this_is_it =
                    ability.cost().parts().iter().any(|part| {
                        matches!(part, CostPart::This(crate::cost::CostAction::Sacrifice))
                    });
                let body = self.body(ability.body());
                self.this_is_it = false;
                let mut text = format!("{cost}: {}", sentences(&body));
                match ability.timing() {
                    Timing::Sorcery => text.push_str(" Activate only as a sorcery."),
                    Timing::YourTurn => text.push_str(" Activate only during your turn."),
                    Timing::Instant | Timing::AsInstant => {}
                }
                return text;
            }
            AbilityKind::Mana => {
                let cost = self.cost_text(ability.cost());
                let mut text = format!("{cost}: {}", sentences(&self.body(ability.body())));
                if let Some(&filter) = ability.only_if_you_control() {
                    let noun = self
                        .noun(filter, Kind::Permanent)
                        .indefinite(Kind::Permanent);
                    text.push_str(" Activate only if you control ");
                    text.push_str(&noun);
                    text.push('.');
                }
                if let Some(restriction) = ability.spend() {
                    text.push(' ');
                    text.push_str(&self.spend_only(restriction));
                }
                if ability.timing() == Timing::AsInstant {
                    text.push_str(" Activate only as an instant.");
                }
                return text;
            }
            AbilityKind::Triggered => self.triggered(ability),
            AbilityKind::Static => self.static_clauses(ability),
            AbilityKind::Replacement => self.replacement(ability),
            AbilityKind::ManaChange => {
                let change = ability.changes_mana().expect("mana change ability");
                vec![self.mana_change_clause(change)]
            }
        };
        sentences(&clauses)
    }

    /// What a spell or an ability does: its effect's clauses, or a marked placeholder for a
    /// Rust body, which has no text to render.
    pub(super) fn body(&mut self, body: Behavior) -> Vec<String> {
        match body {
            Behavior::Data(effect) => self.effect(effect),
            Behavior::Custom(_) => vec!["[custom ability]".to_owned()],
        }
    }

    /// A triggered ability's clauses: "when this creature enters, if it was kicked, gain
    /// control of ..." ([CR 603.1](https://yawgatog.com/resources/magic-rules/#R6031)):
    /// "Triggered abilities have a trigger condition and an effect. They are written as
    /// '[When/Whenever/At] [trigger condition or event], [effect]. [Instructions (if
    /// any).]'"
    pub(super) fn triggered(&mut self, ability: &Ability) -> Vec<String> {
        let Some(trigger) = ability.trigger() else {
            return self.body(ability.body());
        };
        let mut lead = self.trigger_text(trigger);
        if let Some(&condition) = ability.condition() {
            self.this_is_it = matches!(trigger.subject, Subject::This);
            let condition = self.condition(condition);
            self.this_is_it = false;
            lead.push_str(", if ");
            lead.push_str(&condition);
        }
        // A state trigger about the object names it, so its instructions say "it": "When
        // Dark Depths has no ice counters on it, sacrifice it."
        self.this_is_it = matches!(
            trigger.event,
            TriggerEvent::State(&Condition::Compare(Amount::Counters(Objects::This, _), ..))
        );
        let mut clauses = self.body(ability.body());
        self.this_is_it = false;
        match clauses.first_mut() {
            Some(first) => *first = format!("{lead}, {}", super::words::uncapitalized(first)),
            None => clauses.push(lead),
        }
        clauses
    }

    /// The whole of an ability that a keyword names, as its keyword: "Crew 5", "Cycling
    /// {3}", "Dredge 3" ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a),
    /// [CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a),
    /// [CR 702.52a](https://yawgatog.com/resources/magic-rules/#R70252a)).
    pub(super) fn keyword_line(ability: &Ability) -> Option<String> {
        if let Some(&Replacement {
            rewrite: Rewrite::Dredge(n),
            ..
        }) = ability.replacement()
        {
            return Some(format!("Dredge {n}"));
        }
        // Cascade's whole ability (CR 702.85a).
        if let Behavior::Data(effect) = ability.body()
            && std::ptr::eq(effect, &raw const crate::ability::CASCADE)
            && ability
                .trigger()
                .is_some_and(|trigger| trigger.is_this(TriggerEvent::Casts))
        {
            return Some("Cascade".to_owned());
        }
        // Prowess's whole ability (CR 702.108a).
        if let Behavior::Data(effect) = ability.body()
            && std::ptr::eq(effect, &raw const crate::ability::PROWESS_BONUS)
            && ability
                .trigger()
                .is_some_and(|trigger| trigger.event == TriggerEvent::Casts)
        {
            return Some("Prowess".to_owned());
        }
        // Storm's whole ability (CR 702.40a).
        if let Behavior::Data(Effect::CopySpell {
            spells: Objects::This,
            count: Amount::SpellsCastBefore,
            except: None,
            new_targets: true,
        }) = ability.body()
            && ability
                .trigger()
                .is_some_and(|trigger| trigger.is_this(TriggerEvent::Casts))
        {
            return Some("Storm".to_owned());
        }
        if ability.kind() != AbilityKind::Activated {
            return None;
        }
        let cost = ability.cost();
        // Level up's whole ability (CR 702.87a).
        if ability.is_level_up() && cost.parts().is_empty() {
            return Some(format!("Level up {}", mana_text(cost.mana)));
        }
        if let Some(Measure::TotalPower(n)) = cost.measure() {
            return Some(format!("Crew {n}"));
        }
        // Equip's whole ability (CR 702.6a): "[Cost]: Attach this permanent to target creature
        // you control. Activate only as a sorcery."
        let equips = matches!(
            ability.body(),
            Behavior::Data(Effect::Attach {
                objects: Objects::This,
                to: Objects::Target(_),
            })
        ) && ability.timing() == Timing::Sorcery
            && cost.parts().is_empty();
        if equips {
            return Some(format!("Equip {}", mana_text(cost.mana)));
        }
        let draws_one = matches!(
            ability.body(),
            Behavior::Data(Effect::Draw {
                player: Players::You,
                count: Amount::Fixed(1),
            })
        );
        (cost.parts() == [CostPart::DISCARD] && draws_one)
            .then(|| format!("Cycling {}", mana_text(cost.mana)))
    }

    /// A static ability's clauses: what its continuous effect does, or the rule it changes,
    /// "as long as" its condition holds
    /// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041)).
    fn static_clauses(&mut self, ability: &Ability) -> Vec<String> {
        let (condition, text) = if let Some(layer) = ability.layer() {
            (layer.condition, self.layered(layer))
        } else if let Some(rule) = ability.rule() {
            (ability.condition().copied(), self.rule_change(rule))
        } else if let Some(spells) = ability.graveyard_cast() {
            let noun = self.noun(*spells, Kind::Spell).singular(Kind::Spell);
            (
                None,
                format!(
                    "once during each of your turns, you may cast {} from your graveyard",
                    with_article(&noun)
                ),
            )
        } else if let Some(quality) = ability.bands_with() {
            let noun = self.noun(*quality, Kind::Permanent).plural(Kind::Permanent);
            (None, format!("bands with other {noun}"))
        } else if ability.permits_cast_while_searching() {
            (
                None,
                "while you're searching your library, you may cast this card from your library"
                    .to_owned(),
            )
        } else {
            (None, String::new())
        };
        match condition {
            Some(condition) => {
                let condition = self.condition(condition);
                vec![format!("as long as {condition}, {text}")]
            }
            None => vec![text],
        }
    }

    /// A companion's condition: "each permanent card in your starting deck has mana value 2
    /// or less". What every card must pass is said after "has" when it is only words after
    /// the noun ("card with mana value 2 or less"), and as "is a [noun]" otherwise.
    fn deck_condition(&mut self, condition: &crate::DeckCondition) -> String {
        let kind = Kind::Card(None);
        let each = self.noun(condition.each, kind).singular(kind);
        let has = self.noun(condition.has, kind).singular(kind);
        let has = match has.strip_prefix("card with ") {
            Some(rest) => format!("has {rest}"),
            None => format!("is {}", with_article(&has)),
        };
        format!("each {each} in your starting deck {has}")
    }

    /// What a static ability's continuous effect does to the objects it affects: "Adeline's
    /// power is equal to the number of creatures you control", "enchanted permanent has crew
    /// 5" ([CR 611.3](https://yawgatog.com/resources/magic-rules/#R6113)).
    fn layered(&mut self, layer: &LayerSpec) -> String {
        let plural = self.is_plural(layer.affected);
        let mut who = self.objects(layer.affected);
        // A static ability over the permanents some players control names them with no
        // "all": "creatures you control get +1/+1", but "all creatures have flying".
        if let Objects::All {
            zone: Zone::Battlefield,
            filter,
        } = layer.affected
            && super::references::mentions_owner(filter)
            && let Some(rest) = who.strip_prefix("all ")
        {
            who = rest.to_owned();
        }
        // One effect with parts in several layers is one sentence, its subject once: "all
        // creatures lose all abilities and have base power and toughness 1/1".
        let mut clauses = Vec::new();
        let mut predicates = Vec::new();
        for &modification in layer.modifications {
            match self.layered_part(modification, &who, plural) {
                Part::Clause(clause) => clauses.push(clause),
                Part::Predicate(predicate) => predicates.push(predicate),
            }
        }
        if !predicates.is_empty() {
            clauses.push(format!("{who} {}", list(&predicates, "and")));
        }
        list(&clauses, "and")
    }

    /// One part of a static ability's continuous effect, said of `who`.
    fn layered_part(&mut self, modification: Modification, who: &str, plural: bool) -> Part {
        let verb =
            |single: &'static str, several: &'static str| if plural { several } else { single };
        let (has, is) = (verb("has", "have"), verb("is", "are"));
        Part::Predicate(match modification {
            Modification::Control => return Part::Clause(format!("you control {who}")),
            Modification::SetPower(amount) => {
                return Part::Clause(format!(
                    "{} power is equal to {}",
                    possessive(who),
                    self.amount(*amount)
                ));
            }
            Modification::SetPt(power, toughness) => {
                // "Its toughness is equal to that number plus 1": a toughness said from the
                // power names it as "that number".
                let toughness = match *toughness {
                    Amount::Plus(n, of) if of == power => format!("that number plus {n}"),
                    other @ (Amount::Fixed(_)
                    | Amount::X
                    | Amount::ThatMany
                    | Amount::Carried
                    | Amount::ThoseOf(_)
                    | Amount::Count(_)
                    | Amount::Times(..)
                    | Amount::Half { .. }
                    | Amount::Plus(..)
                    | Amount::CardTypesAmong(_)
                    | Amount::Power(_)
                    | Amount::Toughness(_)
                    | Amount::ManaValue(_)
                    | Amount::Counters(..)
                    | Amount::Life(_)
                    | Amount::Most(..)
                    | Amount::SpellsCast(_)
                    | Amount::CardsDrawn(_)
                    | Amount::SpellsCastBefore) => self.amount(other),
                };
                return Part::Clause(format!(
                    "{} power is equal to {} and its toughness is equal to {toughness}",
                    possessive(who),
                    self.amount(*power)
                ));
            }
            Modification::AddTypes(set) => {
                format!("{is} {} in addition to its other types", a_types(set))
            }
            Modification::RemoveTypes(set) => format!("{is}n't {}", a_types(set)),
            Modification::SetTypes(set) => {
                format!("{is} {} and loses all other card types", a_types(set))
            }
            Modification::AddSubtypeWord(word) => {
                let subtype = word.read(&self.words);
                format!(
                    "{is} {} in addition to its other {}",
                    with_article(subtype.name()),
                    other_types(subtype)
                )
            }
            Modification::SetSubtypeWord(word) => {
                format!("{is} {}", with_article(word.read(&self.words).name()))
            }
            Modification::SetLandTypeWords(set) => {
                format!("{is} {}", land_types(set.read(&self.words), plural))
            }
            Modification::AddKeywords(set) => format!("{has} {}", list(&keywords(set), "and")),
            Modification::AddProtection(from) => {
                format!("{has} {}", protection(from, &self.words))
            }
            Modification::Grant(ix) => format!("{has} {}", self.granted(ix)),
            Modification::GrantEscape => format!(
                "{has} escape. The escape cost is equal to the card's mana cost plus {}",
                list(&self.cost_actions(Cost::of_parts(&ESCAPE_EXILES)), "and")
            ),
            Modification::ModifyPt(power, toughness) => {
                format!("{} {}", verb("gets", "get"), pt_change(power, toughness))
            }
            Modification::LoseAllAbilities => format!("{} all abilities", verb("loses", "lose")),
            Modification::LoseAbility(ix) => {
                format!("{} {}", verb("loses", "lose"), self.own_ability(ix))
            }
            Modification::SetColorWords(set) => {
                format!("{is} {}", list(&colors(set.read(&self.words)), "and"))
            }
            Modification::AddColorWords(set) => format!(
                "{is} {} in addition to its other colors",
                list(&colors(set.read(&self.words)), "and")
            ),
            Modification::AddChosenColor => super::modify::chosen_color(is, plural),
            Modification::SetBasePt(power, toughness) => {
                format!("{has} {}", self.base_pt(*power, *toughness))
            }
            Modification::SwitchPt => {
                format!(
                    "{has} {} power and toughness switched",
                    verb("its", "their")
                )
            }
        })
    }

    /// "Base power and toughness 1/1", or the amounts in words when either is not a number
    /// ([CR 613.4b](https://yawgatog.com/resources/magic-rules/#R6134b)).
    pub(super) fn base_pt(&mut self, power: Amount, toughness: Amount) -> String {
        match (power, toughness) {
            (Amount::Fixed(p), Amount::Fixed(t)) => format!("base power and toughness {p}/{t}"),
            (p, t) if p == t => format!(
                "base power and base toughness each equal to {}",
                self.amount(p)
            ),
            (p, t) => format!(
                "base power {} and base toughness {}",
                self.amount(p),
                self.amount(t)
            ),
        }
    }

    /// The ability at `ix` of the face being rendered, in quotation marks, as "loses" names
    /// it.
    pub(super) fn own_ability(&mut self, ix: crate::AbilityIx) -> String {
        match self.face.and_then(|face| face.abilities.get(ix.index())) {
            Some(ability) => match Ctx::keyword_line(ability) {
                Some(keyword) => super::words::uncapitalized(&keyword),
                None => match self.enchant(ability) {
                    Some(enchant) => format!("\"{}\"", super::words::uncapitalized(&enchant)),
                    None => "that ability".to_owned(),
                },
            },
            None => "[an ability]".to_owned(),
        }
    }

    /// An ability a continuous effect grants: a keyword as its keyword ("crew 5"), any
    /// other in quotation marks.
    pub(super) fn granted(&mut self, ix: GrantIx) -> String {
        let Some(ability) = self.face.and_then(|face| face.grants.get(ix.index())) else {
            return "[granted ability]".to_owned();
        };
        self.quoted(ability)
    }

    /// An ability another object has, as a granted ability or a token's ability is written: a
    /// keyword as it is, anything else in quotation marks.
    pub(super) fn quoted(&mut self, ability: &Ability) -> String {
        if let Some(text) = self.designation_paragraph(ability) {
            return text;
        }
        if let Some(keyword) = Ctx::keyword_line(ability) {
            return super::words::uncapitalized(&keyword);
        }
        let saved = (
            self.targets,
            std::mem::take(&mut self.mentioned),
            self.trigger,
            self.ability,
        );
        let text = self.paragraph(ability);
        (self.targets, self.mentioned, self.trigger, self.ability) = saved;
        format!("\"{text}\"")
    }
}

/// Clauses as sentences: each capitalized and closed, joined by spaces.
pub(super) fn sentences(clauses: &[String]) -> String {
    clauses
        .iter()
        .map(|clause| sentence(clause))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Card types with their article: "an artifact creature".
/// One part of a static ability's continuous effect as words: a whole clause, or what is
/// said of the permanents it affects, which several parts share one subject for.
enum Part {
    Clause(String),
    Predicate(String),
}

pub(super) fn a_types(set: crate::Types) -> String {
    with_article(&types(set).join(" "))
}

/// "Its" for "it", or the phrase with "'s".
pub(super) fn possessive(who: &str) -> String {
    if who == "it" {
        "its".to_owned()
    } else {
        format!("{who}'s")
    }
}

/// Whether an ability is part of a keyword that a casting option names, whose line says
/// it: the impending ability's "isn't a creature" and its countdown, offspring's copy, and
/// evoke's sacrifice ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a),
/// [CR 702.175a](https://yawgatog.com/resources/magic-rules/#R702175a),
/// [CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)).
pub(super) fn part_of_keyword(ability: &Ability) -> bool {
    let keyword = |condition: &Condition| {
        condition.any(&|c| {
            matches!(
                c,
                Condition::Paid(
                    CastingKind::Impending | CastingKind::Offspring | CastingKind::Evoke
                )
            )
        })
    };
    ability.condition().is_some_and(keyword)
        || ability
            .layer()
            .and_then(|layer| layer.condition.as_ref())
            .is_some_and(keyword)
}

/// What a subtype is added to: "its other land types" for a land type, "its other types"
/// for any other.
pub(super) fn other_types(subtype: crate::Subtype) -> &'static str {
    if subtype.card_type() == crate::Types::LAND {
        "land types"
    } else {
        "types"
    }
}

/// Basic land types as a land becomes them: "a Mountain", or "Mountains" for several lands.
pub(super) fn land_types(set: crate::BasicLandTypes, plural: bool) -> String {
    let names: Vec<String> = set
        .subtypes()
        .map(|subtype| match (plural, subtype.name()) {
            (false, name) => with_article(name),
            (true, name) if name.ends_with('s') => name.to_owned(),
            (true, name) => format!("{name}s"),
        })
        .collect();
    list(&names, "and")
}
