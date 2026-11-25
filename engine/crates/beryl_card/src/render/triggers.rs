//! The template of when a triggered ability triggers ([`Trigger`]): "when this creature
//! enters", "whenever you cast a noncreature spell", "at the beginning of your upkeep".

use super::Ctx;
use super::designations::roman;
use super::filters::Kind;
use super::words::step_name;
use crate::effect::{Amount, Comparison, Condition, Objects, Players};
use crate::trigger::{Subject, Trigger, TriggerEvent};

impl Ctx {
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
            TriggerEvent::ClassLevel(level) => format!("when this Class becomes level {level}"),
            TriggerEvent::Chapter { first, last } => {
                (first..=last).map(roman).collect::<Vec<_>>().join(", ")
            }
            TriggerEvent::Enters
            | TriggerEvent::Dies
            | TriggerEvent::Leaves
            | TriggerEvent::Attacks
            | TriggerEvent::BecomesTarget
            | TriggerEvent::BecomesTargetOfSpell
            | TriggerEvent::Mutates
            | TriggerEvent::DealsDamage
            | TriggerEvent::TurnedFaceUp
            | TriggerEvent::DealsCombatDamageToPlayer => {
                let (word, subject) = match trigger.subject {
                    Subject::This => (
                        if matches!(
                            trigger.event,
                            TriggerEvent::Attacks
                                | TriggerEvent::DealsCombatDamageToPlayer
                                | TriggerEvent::DealsDamage
                                | TriggerEvent::BecomesTargetOfSpell
                                | TriggerEvent::Mutates
                        ) {
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
            TriggerEvent::FirstStep(step) => {
                let step = self.whose_step(trigger, &format!("first {}", step_name(step)));
                format!("at the beginning of {step} each turn")
            }
            TriggerEvent::Step(step) => format!(
                "at the beginning of {}",
                self.whose_step(trigger, step_name(step))
            ),
            TriggerEvent::MainPhase => format!(
                "at the beginning of {}",
                self.whose_step(trigger, "main phase")
            ),
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
            TriggerEvent::Casts => self.cast_trigger_text(trigger, &player_verb("cast"), None),
            TriggerEvent::CastsFrom(zone) => {
                self.cast_trigger_text(trigger, &player_verb("cast"), Some(zone))
            }
            TriggerEvent::Discards => format!("whenever {} a card", player_verb("discard")),
            TriggerEvent::Draws => format!("whenever {} a card", player_verb("draw")),
            // "When Dark Depths has no ice counters on it": the object is the subject.
            TriggerEvent::State(&Condition::Compare(
                Amount::Counters(Objects::This, kind),
                Comparison::Equal | Comparison::AtMost,
                Amount::Fixed(0),
            )) => format!("when {} has no {} counters on it", self.this, kind.name()),
            TriggerEvent::State(&condition) => format!("when {}", self.condition(condition)),
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
            TriggerEvent::BecomesTarget => "becomes the target of a spell or ability".to_owned(),
            TriggerEvent::BecomesTargetOfSpell => "becomes the target of a spell".to_owned(),
            TriggerEvent::Mutates => "mutates".to_owned(),
            TriggerEvent::DealsDamage => "deals damage".to_owned(),
            TriggerEvent::TurnedFaceUp => "is turned face up".to_owned(),
            TriggerEvent::DealsCombatDamageToPlayer => {
                let whom = match trigger.whose {
                    Players::EachPlayer => "a player".to_owned(),
                    Players::EachOpponent => "an opponent".to_owned(),
                    other @ (Players::You
                    | Players::Target(_)
                    | Players::ControllerOf(_)
                    | Players::OwnerOf(_)
                    | Players::Enchanted
                    | Players::ThatPlayer
                    | Players::SubgameNonwinners
                    | Players::EachOther(_)) => self.player(other),
                };
                format!("deals combat damage to {whom}")
            }
            TriggerEvent::PlayerAttacks
            | TriggerEvent::Step(_)
            | TriggerEvent::FirstStep(_)
            | TriggerEvent::MainPhase
            | TriggerEvent::CreatesTokens
            | TriggerEvent::Casts
            | TriggerEvent::CastsFrom(_)
            | TriggerEvent::Discards
            | TriggerEvent::Draws
            | TriggerEvent::ClassLevel(_)
            | TriggerEvent::Chapter { .. }
            | TriggerEvent::State(_) => String::new(),
        }
    }

    /// The player a trigger is about, and whether the verb after takes an "s": "you",
    /// "an opponent", "a player".
    fn trigger_player(&mut self, whose: Players) -> (String, bool) {
        match whose {
            Players::You => ("you".to_owned(), false),
            Players::EachOpponent => ("an opponent".to_owned(), true),
            Players::EachPlayer => ("a player".to_owned(), true),
            other @ (Players::Target(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::Enchanted
            | Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::EachOther(_)) => (self.player(other), true),
        }
    }

    /// Whose step it is: "your upkeep", "each opponent's upkeep", "each end step", and for
    /// a delayed trigger "the next end step", "your next main phase" or "that turn's end step"
    /// (CR 603.7).
    fn whose_step(&mut self, trigger: &Trigger, step: &str) -> String {
        if trigger.exact_turn {
            return format!("that turn's {step}");
        }
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
}
