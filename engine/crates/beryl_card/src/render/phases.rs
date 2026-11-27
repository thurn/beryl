//! Templates for instructions about turns and what happens later in them: a delayed
//! triggered ability, an extra turn, phase or step, an emblem, and control of a player's turn.

use super::Ctx;
use crate::TurnPhase;
use crate::ability::Behavior;
use crate::definition::CardDef;
use crate::effect::{Condition, Effect, Objects, Players};
use crate::index::AbilityIx;
use crate::trigger::TriggerEvent;

impl Ctx {
    /// The delayed triggered ability an [`Effect::Delay`] makes, from its face.
    pub(super) fn delay(&mut self, ability: AbilityIx) -> Vec<String> {
        let delayed = self
            .face
            .and_then(|face| face.abilities.get(ability.index()));
        match delayed {
            Some(delayed) => self.delayed(delayed),
            None => vec!["[delayed ability]".to_owned()],
        }
    }

    /// A reflexive triggered ability made in the middle of an effect ([`Effect::Reflexive`]):
    /// "when a creature card is exiled this way, put a +1/+1 counter on target creature you
    /// control" (CR 603.12). The condition is about what the instruction before did, so it is
    /// written before the text turns to the reflexive ability's own targets.
    pub(super) fn reflexive(
        &mut self,
        condition: Option<Condition>,
        ability: AbilityIx,
    ) -> Vec<String> {
        let lead = match condition {
            Some(condition) => self.reflexive_condition(condition),
            None => "you do".to_owned(),
        };
        let reflexive = self
            .face
            .and_then(|face| face.abilities.get(ability.index()));
        let Some(reflexive) = reflexive else {
            return vec![format!("when {lead}, [reflexive ability]")];
        };
        let saved = (
            self.targets,
            std::mem::take(&mut self.mentioned),
            self.trigger,
            self.subject,
            self.group,
        );
        self.begin(reflexive.targets(), reflexive.trigger());
        let mut clauses = self.body(reflexive.body());
        (
            self.targets,
            self.mentioned,
            self.trigger,
            self.subject,
            self.group,
        ) = saved;
        match clauses.first_mut() {
            Some(first) => *first = format!("when {lead}, {}", super::words::uncapitalized(first)),
            None => clauses.push(format!("when {lead}")),
        }
        clauses
    }

    /// The condition of a reflexive ability: "a creature card is exiled this way" of what the
    /// instruction before moved, and otherwise the condition as it follows "if".
    fn reflexive_condition(&mut self, condition: Condition) -> String {
        if let (Condition::Is(Objects::Those, filter), Some(done)) = (condition, self.done_this_way)
        {
            let kind = super::filters::Kind::Card(None);
            let noun = self.noun(filter, kind).indefinite(kind);
            return format!("{noun} is {done} this way");
        }
        self.condition(condition)
    }

    /// A delayed triggered ability made in the middle of an effect: "at the beginning of the
    /// next end step, untap up to two lands" (CR 603.7a).
    fn delayed(&mut self, ability: &'static crate::Ability) -> Vec<String> {
        let saved = (
            self.targets,
            std::mem::take(&mut self.mentioned),
            self.trigger,
            self.subject,
            self.group,
        );
        self.begin(ability.targets(), ability.trigger());
        let clauses = match (ability.trigger(), ability.body()) {
            (Some(trigger), Behavior::Data(effect))
                if names_carried(effect) && matches!(trigger.event, TriggerEvent::Step(_)) =>
            {
                let mut clauses = self.body(ability.body());
                let lead = self.trigger_text(trigger);
                if let Some(last) = clauses.last_mut() {
                    *last = format!("{last} {}", super::words::uncapitalized(&lead));
                }
                clauses
            }
            (Some(_) | None, Behavior::Data(_) | Behavior::Custom(_)) => self.triggered(ability),
        };
        (
            self.targets,
            self.mentioned,
            self.trigger,
            self.subject,
            self.group,
        ) = saved;
        clauses
    }

    /// What the delayed ability being written carries, as a noun: "creature" for the
    /// creature an Aura put onto the battlefield ("that creature's controller sacrifices
    /// it"), and "permanent" when no instruction of the face says what it carries.
    pub(super) fn carried_noun(&self) -> &'static str {
        let carried = std::cell::Cell::new(None);
        if let (Some(face), Some(trigger)) = (self.face, self.trigger) {
            let carries_this = |ix: AbilityIx| {
                face.abilities
                    .get(ix.index())
                    .and_then(crate::Ability::trigger)
                    .is_some_and(|delayed| std::ptr::eq(delayed, trigger))
            };
            for ability in face.abilities {
                if let Behavior::Data(effect) = ability.body() {
                    effect.any(&|instruction| {
                        if let Effect::Delay {
                            ability,
                            object: Some(object),
                            ..
                        } = *instruction
                            && carries_this(ability)
                        {
                            carried.set(Some(object));
                        }
                        false
                    });
                }
            }
        }
        match carried.get() {
            Some(Objects::PutOntoBattlefieldWithThis) => "creature",
            Some(_) | None => "permanent",
        }
    }

    /// "Take an extra turn after this one", and the delayed ability that comes with it, such
    /// as "at the beginning of that turn's end step, you lose the game".
    pub(super) fn extra_turn(
        &mut self,
        player: Players,
        ability: Option<AbilityIx>,
    ) -> Vec<String> {
        let take_turn = self.clause(player, ("take", "takes"), |_| {
            "an extra turn after this one".to_owned()
        });
        let mut clauses = vec![take_turn];
        if let Some(trigger) = ability.and_then(|ability| {
            self.face
                .and_then(|face| face.abilities.get(ability.index()))
        }) {
            clauses.extend(self.delayed(trigger));
        }
        clauses
    }

    /// "You get an emblem with "..."".
    pub(super) fn get_emblem(&mut self, player: Players, emblem: &'static CardDef) -> Vec<String> {
        vec![self.stated(player, ("get", "gets"), |_| {
            format!("an emblem with \"{}\"", emblem_text(emblem))
        })]
    }

    /// "You control target player during that player's next turn".
    pub(super) fn control_player(&mut self, player: Players) -> Vec<String> {
        vec![format!(
            "you control {} during that player's next turn",
            self.player(player)
        )]
    }
}

/// "That player gets an additional combat step after this step".
pub(super) fn additional_step(step: crate::TurnStep) -> Vec<String> {
    vec![format!(
        "that player gets an additional {} after this step",
        super::words::step_noun(step),
    )]
}

/// "After this main phase, there is an additional combat phase followed by an additional
/// main phase".
pub(super) fn additional_phases(after: TurnPhase, phases: &[TurnPhase]) -> Vec<String> {
    if phases.is_empty() {
        return Vec::new();
    }
    let added = phases
        .iter()
        .map(|&phase| format!("an additional {} phase", name(phase)))
        .collect::<Vec<_>>()
        .join(" followed by ");
    vec![format!(
        "after this {} phase, there is {added}",
        name(after)
    )]
}

fn name(phase: TurnPhase) -> &'static str {
    match phase {
        TurnPhase::Beginning => "beginning",
        TurnPhase::Main => "main",
        TurnPhase::Combat => "combat",
        TurnPhase::Ending => "ending",
    }
}

/// Whether a delayed ability's one instruction acts on the object it carries, which Oracle
/// text says before a step's trigger: "Return that card to the battlefield under its owner's
/// control at the beginning of the next end step."
fn names_carried(effect: &Effect) -> bool {
    effect.operands().objects == Some(&Objects::Carried)
}

/// The text of an emblem's abilities, as the quoted text of "an emblem with "..."".
fn emblem_text(emblem: &'static CardDef) -> String {
    let Some(face) = emblem.faces.first() else {
        return String::new();
    };
    let mut ctx = Ctx::of_face(face, "this emblem");
    face.abilities
        .iter()
        .map(|ability| ctx.paragraph(ability))
        .collect::<Vec<_>>()
        .join(" ")
}
