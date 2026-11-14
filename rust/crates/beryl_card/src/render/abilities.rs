//! The templates of an ability's parts: its cost ([`Cost`]), its trigger ([`Trigger`]),
//! what a static ability does ([`LayerSpec`], [`RuleChange`]), a replacement
//! ([`Replacement`]), and the whole ability as one paragraph.

use super::Ctx;
use super::effects::says_nothing;
use super::filters::{Kind, zone_name};
use super::words::{
    capitalized, counted, keywords, list, mana_text, number, pt_change, sentence, types,
    with_article,
};
use crate::ability::{Ability, AbilityKind, Behavior, Timing};
use crate::continuous::{Governed, LayerSpec, Modification, Replacement, RuleChange, When};
use crate::cost::{CastingKind, Chosen, Cost, CostAction, CostPart, Measure};
use crate::effect::{Amount, Condition, Effect, Objects, Players};
use crate::ids::{TurnStep, Zone};
use crate::index::GrantIx;
use crate::trigger::{Subject, Trigger, TriggerEvent};

impl Ctx {
    /// One ability as its paragraph of rules text, each clause a sentence.
    pub(super) fn paragraph(&mut self, ability: &Ability) -> String {
        self.begin(ability.targets(), ability.trigger());
        if let Some(keyword) = Ctx::keyword_line(ability) {
            return keyword;
        }
        let clauses = match ability.kind() {
            AbilityKind::Spell => {
                if says_nothing(ability.body())
                    && let Some(&spec) = ability.targets().first()
                {
                    // An Aura's spell targets what it will enchant
                    // ([CR 303.4a](https://yawgatog.com/resources/magic-rules/#R3034a)).
                    return format!("Enchant {}", self.target_noun(spec));
                }
                self.body(ability.body())
            }
            AbilityKind::Activated => {
                let cost = self.cost_text(ability.cost());
                let mut text = format!("{cost}: {}", sentences(&self.body(ability.body())));
                if ability.timing() == Timing::Sorcery {
                    text.push_str(" Activate only as a sorcery.");
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
                return text;
            }
            AbilityKind::Triggered => self.triggered(ability),
            AbilityKind::Static => self.static_clauses(ability),
            AbilityKind::Replacement => self.replacement(ability),
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
        let mut clauses = self.body(ability.body());
        match clauses.first_mut() {
            Some(first) => *first = format!("{lead}, {}", super::words::uncapitalized(first)),
            None => clauses.push(lead),
        }
        clauses
    }

    /// When a triggered ability triggers: "when this creature enters", "whenever you cast a
    /// noncreature spell", "at the beginning of your upkeep"
    /// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)).
    pub(super) fn trigger_text(&mut self, trigger: &Trigger) -> String {
        let (who, third) = self.trigger_player(trigger.whose);
        let player_verb = |base: &str| {
            if third {
                format!("{who} {base}s")
            } else {
                format!("{who} {base}")
            }
        };
        match trigger.event {
            TriggerEvent::Enters
            | TriggerEvent::Dies
            | TriggerEvent::Leaves
            | TriggerEvent::Attacks
            | TriggerEvent::DealsCombatDamageToPlayer => {
                let (word, subject) = match trigger.subject {
                    Subject::This => (
                        if matches!(trigger.event, TriggerEvent::Attacks)
                            || matches!(trigger.event, TriggerEvent::DealsCombatDamageToPlayer)
                        {
                            "whenever"
                        } else {
                            "when"
                        },
                        self.this.clone(),
                    ),
                    Subject::A(filter) => (
                        "whenever",
                        self.noun(filter, Kind::Permanent)
                            .indefinite(Kind::Permanent),
                    ),
                };
                format!("{word} {subject} {}", self.event_verb(trigger))
            }
            TriggerEvent::PlayerAttacks => match trigger.subject {
                Subject::A(crate::Filter::Any) | Subject::This => {
                    format!("whenever {}", player_verb("attack"))
                }
                Subject::A(filter) => {
                    let attackers = self.noun(filter, Kind::Permanent).plural(Kind::Permanent);
                    format!(
                        "whenever {} with one or more {attackers}",
                        player_verb("attack")
                    )
                }
            },
            TriggerEvent::Step(step) => {
                let step = step_name(step);
                format!("at the beginning of {}", self.whose_step(trigger, step))
            }
            TriggerEvent::MainPhase => {
                format!(
                    "at the beginning of {}",
                    self.whose_step(trigger, "main phase")
                )
            }
            TriggerEvent::CreatesTokens => {
                let tokens = match trigger.subject {
                    Subject::A(crate::Filter::Any) | Subject::This => "tokens".to_owned(),
                    Subject::A(filter) => {
                        let noun = self.noun(filter, Kind::Permanent).singular(Kind::Permanent);
                        format!("{noun} tokens")
                    }
                };
                format!("whenever {} one or more {tokens}", player_verb("create"))
            }
            TriggerEvent::Casts => {
                let spell = match trigger.subject {
                    Subject::A(filter) => self.noun(filter, Kind::Spell).indefinite(Kind::Spell),
                    Subject::This => self.this.clone(),
                };
                format!("whenever {} {spell}", player_verb("cast"))
            }
            TriggerEvent::Discards => format!("whenever {} a card", player_verb("discard")),
            TriggerEvent::Draws => format!("whenever {} a card", player_verb("draw")),
        }
    }

    /// What happens to the object: "enters", "dies", "leaves the battlefield", "attacks", "deals combat damage to a
    /// player".
    pub(super) fn event_verb(&mut self, trigger: &Trigger) -> String {
        match trigger.event {
            TriggerEvent::Enters => "enters".to_owned(),
            TriggerEvent::Dies => "dies".to_owned(),
            TriggerEvent::Leaves => "leaves the battlefield".to_owned(),
            TriggerEvent::Attacks => "attacks".to_owned(),
            TriggerEvent::DealsCombatDamageToPlayer => {
                let whom = match trigger.whose {
                    Players::EachPlayer => "a player".to_owned(),
                    Players::EachOpponent => "an opponent".to_owned(),
                    other => self.player(other),
                };
                format!("deals combat damage to {whom}")
            }
            TriggerEvent::PlayerAttacks
            | TriggerEvent::Step(_)
            | TriggerEvent::MainPhase
            | TriggerEvent::CreatesTokens
            | TriggerEvent::Casts
            | TriggerEvent::Discards
            | TriggerEvent::Draws => String::new(),
        }
    }

    /// The player a trigger is about, and whether the verb after takes an "s": "you",
    /// "an opponent", "a player".
    fn trigger_player(&mut self, whose: Players) -> (String, bool) {
        match whose {
            Players::You => ("you".to_owned(), false),
            Players::EachOpponent => ("an opponent".to_owned(), true),
            Players::EachPlayer => ("a player".to_owned(), true),
            other => (self.player(other), true),
        }
    }

    /// Whose step it is: "your upkeep", "each opponent's upkeep", "each end step", and for
    /// a delayed trigger "the next end step" or "your next main phase" (CR 603.7).
    fn whose_step(&mut self, trigger: &Trigger, step: &str) -> String {
        match (trigger.whose, trigger.delayed) {
            (Players::You, true) => format!("your next {step}"),
            (Players::EachPlayer, true) => format!("the next {step}"),
            (other, true) => format!("{} next {step}", self.possessive(other)),
            (Players::You, false) => format!("your {step}"),
            (Players::EachOpponent, false) => format!("each opponent's {step}"),
            (Players::EachPlayer, false) => format!("each {step}"),
            (other, false) => format!("{} {step}", self.possessive(other)),
        }
    }

    /// A cost as it is written before the colon
    /// ([CR 602.1a](https://yawgatog.com/resources/magic-rules/#R6021a)): "{2}, {T}",
    /// "{T}, Sacrifice this land", "+2", "Waterbend {3}".
    pub(super) fn cost_text(&mut self, cost: &Cost) -> String {
        if let Some(loyalty) = cost.loyalty_change() {
            // A minus sign, as printed, rather than a hyphen.
            return match loyalty {
                0 => "0".to_owned(),
                l if l > 0 => format!("+{l}"),
                l => format!("\u{2212}{}", l.unsigned_abs()),
            };
        }
        let mut parts = Vec::new();
        if matches!(cost.measure(), Some(Measure::ForGeneric(_))) {
            parts.push(format!("Waterbend {}", mana_text(cost.mana)));
        } else if !cost.mana.is_zero() {
            parts.push(mana_text(cost.mana));
        }
        parts.extend(self.cost_actions(*cost).iter().map(|a| capitalized(a)));
        if parts.is_empty() {
            "{0}".to_owned()
        } else {
            parts.join(", ")
        }
    }

    /// What paying a cost does besides its mana, each part as an instruction: "{T}",
    /// "sacrifice this land", "exile six cards from your graveyard", "pay 3 life".
    pub(super) fn cost_actions(&mut self, cost: Cost) -> Vec<String> {
        cost.parts()
            .iter()
            .filter_map(|&part| self.cost_part(part))
            .collect()
    }

    /// One part of a cost as an instruction, or `None` for one the cost says elsewhere: a
    /// loyalty change, or the tapping of a waterbend cost.
    fn cost_part(&mut self, part: CostPart) -> Option<String> {
        Some(match part {
            CostPart::This(CostAction::Tap) => "{T}".to_owned(),
            CostPart::This(CostAction::Sacrifice) => format!("sacrifice {}", self.this),
            CostPart::This(CostAction::Discard) => "discard this card".to_owned(),
            CostPart::This(CostAction::Exile) => format!("exile {}", self.this),
            CostPart::This(CostAction::Reveal) => "reveal this card".to_owned(),
            CostPart::This(CostAction::RemoveCounter(kind)) => {
                format!(
                    "remove {} counter from {}",
                    with_article(kind.name()),
                    self.this
                )
            }
            CostPart::Chosen(action, chosen) => return self.chosen(action, chosen),
            CostPart::Loyalty(_) => return None,
            CostPart::Life(n) => format!("pay {n} life"),
            CostPart::OneOf(costs) => {
                let each: Vec<String> = costs.iter().map(|c| self.cost_text(c)).collect();
                list(&each, "or")
            }
        })
    }

    /// A cost part that acts on objects its payer chooses: "sacrifice another creature",
    /// "exile a blue card from your hand", "tap any number of untapped creatures you
    /// control with total power 5 or more".
    fn chosen(&mut self, action: CostAction, chosen: Chosen) -> Option<String> {
        let verb = match action {
            CostAction::Tap => "tap",
            CostAction::Sacrifice => "sacrifice",
            CostAction::Discard => "discard",
            CostAction::Exile => "exile",
            CostAction::Reveal => "reveal",
            CostAction::RemoveCounter(_) => "remove a counter from",
        };
        let kind = match chosen.zone {
            Zone::Battlefield | Zone::Nowhere | Zone::Stack => Kind::Permanent,
            _ => Kind::Card(None),
        };
        let noun = self.noun(chosen.filter, kind);
        let from = match chosen.zone {
            Zone::Library | Zone::Graveyard | Zone::Exile => {
                format!(" from your {}", zone_name(chosen.zone))
            }
            Zone::Hand if action != CostAction::Discard => " from your hand".to_owned(),
            _ => String::new(),
        };
        let things = match chosen.measure {
            Measure::Count(1) if chosen.other => format!("another {}", noun.singular(kind)),
            Measure::Count(1) => noun.indefinite(kind),
            Measure::Count(n) if chosen.other => {
                format!("{} other {}", number(n), noun.plural(kind))
            }
            Measure::Count(n) => counted(n, &noun.singular(kind), &noun.plural(kind)),
            Measure::TotalPower(n) => format!(
                "any number of {}untapped {} with total power {n} or more",
                if chosen.other { "other " } else { "" },
                noun.plural(kind)
            ),
            Measure::ForGeneric(_) => return None,
        };
        Some(format!("{verb} {things}{from}"))
    }

    /// The whole of an ability that a keyword names, as its keyword: "Crew 5", "Cycling
    /// {3}" ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a),
    /// [CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a)).
    pub(super) fn keyword_line(ability: &Ability) -> Option<String> {
        if ability.kind() != AbilityKind::Activated {
            return None;
        }
        let cost = ability.cost();
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

    /// What a static ability's continuous effect does to the objects it affects: "Adeline's
    /// power is equal to the number of creatures you control", "enchanted permanent has crew
    /// 5" ([CR 611.3](https://yawgatog.com/resources/magic-rules/#R6113)).
    fn layered(&mut self, layer: &LayerSpec) -> String {
        let plural = self.is_plural(layer.affected);
        let has = if plural { "have" } else { "has" };
        let is = if plural { "are" } else { "is" };
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
        match layer.modification {
            Modification::Control => format!("you control {who}"),
            Modification::AddTypes(set) => {
                format!("{who} {is} {} in addition to its other types", a_types(set))
            }
            Modification::RemoveTypes(set) => {
                format!("{who} {}n't {}", is, a_types(set))
            }
            Modification::SetTypes(set) => {
                format!("{who} {is} {} and loses all other card types", a_types(set))
            }
            Modification::AddSubtype(subtype) => format!(
                "{who} {is} {} in addition to its other types",
                with_article(subtype.name())
            ),
            Modification::AddKeywords(set) => {
                format!("{who} {has} {}", list(&keywords(set), "and"))
            }
            Modification::Grant(ix) => format!("{who} {has} {}", self.granted(ix)),
            Modification::SetPower(amount) => {
                format!(
                    "{} power is equal to {}",
                    possessive(&who),
                    self.amount(*amount)
                )
            }
            Modification::ModifyPt(power, toughness) => {
                let gets = if plural { "get" } else { "gets" };
                format!("{who} {gets} {}", pt_change(power, toughness))
            }
        }
    }

    /// What a resolving spell's continuous effect does: "it gains first strike", "gain
    /// control of target artifact", "up to one target creature gets -2/-0"
    /// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)).
    pub(super) fn modify(&mut self, objects: Objects, modification: Modification) -> String {
        if modification == Modification::Control {
            return self.clause(Players::You, ("gain", "gains"), |c| {
                format!("control of {}", c.objects(objects))
            });
        }
        let plural = self.is_plural(objects);
        let verb =
            |single: &'static str, several: &'static str| if plural { several } else { single };
        let who = self.objects(objects);
        let text = match modification {
            Modification::Control => String::new(),
            Modification::AddTypes(set) => {
                format!("{who} {} {}", verb("becomes", "become"), a_types(set))
            }
            Modification::RemoveTypes(set) => {
                format!(
                    "{who} {} {}",
                    verb("stops being", "stop being"),
                    a_types(set)
                )
            }
            Modification::SetTypes(set) => format!(
                "{who} {} {} and {} all other card types",
                verb("becomes", "become"),
                a_types(set),
                verb("loses", "lose")
            ),
            Modification::AddSubtype(subtype) => format!(
                "{who} {} {} in addition to its other types",
                verb("becomes", "become"),
                with_article(subtype.name())
            ),
            Modification::AddKeywords(set) => {
                format!(
                    "{who} {} {}",
                    verb("gains", "gain"),
                    list(&keywords(set), "and")
                )
            }
            Modification::Grant(ix) => {
                format!("{who} {} {}", verb("gains", "gain"), self.granted(ix))
            }
            Modification::SetPower(amount) => {
                format!(
                    "{} power becomes {}",
                    possessive(&who),
                    self.amount(*amount)
                )
            }
            Modification::ModifyPt(power, toughness) => {
                format!(
                    "{who} {} {}",
                    verb("gets", "get"),
                    pt_change(power, toughness)
                )
            }
        };
        self.plural = plural;
        text
    }

    /// An ability a continuous effect grants: a keyword as its keyword ("crew 5"), any
    /// other in quotation marks.
    fn granted(&mut self, ix: GrantIx) -> String {
        let Some(ability) = self.face.and_then(|face| face.grants.get(ix.index())) else {
            return "[granted ability]".to_owned();
        };
        if let Some(keyword) = Ctx::keyword_line(ability) {
            return super::words::uncapitalized(&keyword);
        }
        let saved = (
            self.targets,
            std::mem::take(&mut self.mentioned),
            self.trigger,
        );
        let text = self.paragraph(ability);
        (self.targets, self.mentioned, self.trigger) = saved;
        format!("\"{text}\"")
    }

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
        };
        match (rule.when, rule.what) {
            (When::OnlyAsSorcery, _) => {
                format!("{who} can {what} only any time {they} could cast a sorcery")
            }
            (When::AsInstant, Governed::CastSpells(_)) => {
                format!("{who} may {what} as though they had flash")
            }
            (When::AsInstant, Governed::ActivateLoyaltyAbilities(_)) => {
                format!("{who} may {what} any time {they} could cast an instant")
            }
        }
    }

    /// A replacement effect's clauses, such as "this land enters tapped" or "if you would
    /// draw a card while your library has no cards in it, you win the game instead"
    /// ([CR 614.1](https://yawgatog.com/resources/magic-rules/#R6141)).
    fn replacement(&mut self, ability: &Ability) -> Vec<String> {
        let this = self.this.clone();
        let mut clauses = match ability.replacement() {
            Replacement::None => self.body(ability.body()),
            Replacement::EntersTapped => vec![format!("{this} enters tapped")],
            Replacement::EntersTappedUnlessFewLands(n) => vec![format!(
                "{this} enters tapped unless you control {} or fewer other lands",
                number(n)
            )],
            Replacement::EntersTappedUnlessPayLife(n) => vec![
                format!("as {this} enters, you may pay {n} life"),
                "if you don't, it enters tapped".to_owned(),
            ],
            Replacement::WouldDraw => {
                let condition = ability
                    .condition()
                    .map(|&c| format!(" while {}", self.condition(c)))
                    .unwrap_or_default();
                let mut instead = self.body(ability.body());
                if let Some(first) = instead.first_mut() {
                    let you = if first.starts_with("you ") {
                        ""
                    } else {
                        "you "
                    };
                    *first = format!("if you would draw a card{condition}, {you}{first} instead");
                }
                return instead;
            }
        };
        if let (Some(&condition), false) = (
            ability.condition(),
            matches!(ability.replacement(), Replacement::WouldDraw),
        ) {
            let condition = self.condition(condition);
            if let Some(first) = clauses.first_mut() {
                *first = format!("if {condition}, {first}");
            }
        }
        clauses
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
fn a_types(set: crate::Types) -> String {
    with_article(&types(set).join(" "))
}

/// "Its" for "it", or the phrase with "'s".
fn possessive(who: &str) -> String {
    if who == "it" {
        "its".to_owned()
    } else {
        format!("{who}'s")
    }
}

/// The name of a step or phase as a trigger says it, such as "upkeep" or "end step"
/// ([CR 500.1](https://yawgatog.com/resources/magic-rules/#R5001)).
fn step_name(step: TurnStep) -> &'static str {
    match step {
        TurnStep::Untap => "untap step",
        TurnStep::Upkeep => "upkeep",
        TurnStep::Draw => "draw step",
        TurnStep::PrecombatMain => "precombat main phase",
        TurnStep::BeginningOfCombat => "combat",
        TurnStep::DeclareAttackers => "declare attackers step",
        TurnStep::DeclareBlockers => "declare blockers step",
        TurnStep::FirstStrikeDamage => "first-strike damage step",
        TurnStep::CombatDamage => "combat damage step",
        TurnStep::EndOfCombat => "end of combat step",
        TurnStep::PostcombatMain => "postcombat main phase",
        TurnStep::End => "end step",
        TurnStep::Cleanup => "cleanup step",
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
