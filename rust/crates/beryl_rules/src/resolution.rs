//! [`Resolution`]: the spell or ability that is resolving, and the record its questions are
//! answered from (the crate's contract, "Resolution"; architecture §7.3). The questions
//! themselves are [`crate::choices`]'.

use crate::card::{Ability, CastingIx, FaceAbility, ManaColor, Stop};
use crate::cast;
use crate::decision::{Answers, Choice, Decision, Outcome, StallReason};
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
use crate::profile::Stops;
use crate::state::{
    ObjectRef, PendingTrigger, StackEntry, Target, TargetPlace, TargetPlaces, TriggerInfo,
};

/// One resolution of a spell or ability: what is resolving, and the record its questions are
/// answered from. A library function takes it, beside the [`Game`], only when what it does
/// makes sense only while something resolves: it reads the source or the controller, or it
/// asks.
pub struct Resolution<'a, L: Layout = Small> {
    /// The stack entry resolving: its source, controller, card, face and ability, its
    /// targets, the costs paid for it, X, and what triggered it.
    pub(crate) entry: StackEntry<L>,
    pub(crate) ability: &'static Ability,
    /// Its targets that were legal as the resolution began, settled once for the whole effect
    /// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If the spell or
    /// ability specifies targets, it checks whether the targets are still legal."
    pub(crate) legal: TargetPlaces,
    /// Where questions go: an inline source answers them, anything else pauses the
    /// resolution.
    answers: Option<&'a mut dyn Answers<L>>,
    /// The answers given so far to this resolution's questions, and how many this run of it
    /// has used.
    record: &'a [L::Index],
    used: usize,
    /// The question that paused this run.
    pub(crate) question: Option<Decision<L>>,
    /// For a mana ability that can produce more than one kind of mana, the kind its
    /// activator chose.
    pub(crate) mana: ManaColor,
    /// A card the effect lets a player cast as it finishes resolving (CR 608.2g).
    pub(crate) offer: Option<(ObjectRef<L>, Seat, bool)>,
}

impl<'a, L: Layout> Resolution<'a, L> {
    /// The resolution of `entry`, whose ability is `ability`, answering from `record` and
    /// then `answers`.
    pub(crate) fn new(
        entry: StackEntry<L>,
        ability: &'static Ability,
        legal: TargetPlaces,
        answers: Option<&'a mut dyn Answers<L>>,
        record: &'a [L::Index],
    ) -> Self {
        Resolution {
            entry,
            ability,
            legal,
            answers,
            record,
            used: 0,
            question: None,
            mana: ManaColor::Colorless,
            offer: None,
        }
    }

    /// The resolution of `ability` as what `trigger` describes, with no targets and no
    /// answer source: a question pauses it. A mana ability resolves so, never having been
    /// on the stack ([CR 605.3b](https://yawgatog.com/resources/magic-rules/#R6053b)).
    pub(crate) fn of_trigger(trigger: &PendingTrigger<L>, ability: &'static Ability) -> Self {
        let entry = StackEntry {
            object: trigger.source,
            controller: trigger.controller,
            card: trigger.card,
            face: trigger.face,
            ability: Some(FaceAbility::Own(trigger.ability)),
            info: trigger.info,
            ..StackEntry::default()
        };
        Resolution::new(entry, ability, TargetPlaces::NONE, None, &[])
    }

    /// The spell's card or the ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn source(&self) -> ObjectRef<L> {
        self.entry.object.expect("an ability with a source")
    }

    /// The player who controls the spell or ability
    /// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of
    /// the spell or ability follows its instructions in the order written."
    pub fn controller(&self) -> Seat {
        self.entry.controller
    }

    /// What a triggered ability triggered on, with last-known information for a look-back
    /// trigger.
    pub fn trigger(&self) -> TriggerInfo<L> {
        self.entry.info
    }

    /// The announced value of X.
    pub fn x(&self) -> u8 {
        self.entry.x
    }

    /// Whether the casting option at `option` in the face's list was paid.
    pub fn paid(&self, option: CastingIx) -> bool {
        self.entry.costs.contains(option)
    }

    /// The target chosen at `place` if it is legal now, `None` if it was not chosen or is
    /// illegal ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "Illegal
    /// targets, if any, won't be affected by parts of a resolving spell's effect for which
    /// they're illegal."
    pub fn target(&self, game: &Game<'_, L>, place: TargetPlace) -> Option<Target<L>> {
        let target = *self.entry.targets.as_slice().get(place.index())?;
        cast::still_legal(game.state, game.table, &self.entry, self.ability, target)
            .then_some(target)
    }

    /// The target at `place` as an object, if it is one and legal now.
    pub fn target_object(&self, game: &Game<'_, L>, place: TargetPlace) -> Option<ObjectRef<L>> {
        self.target(game, place).and_then(Target::as_object)
    }

    /// The target at `place` as a player, if it is one and legal now.
    pub fn target_player(&self, game: &Game<'_, L>, place: TargetPlace) -> Option<Seat> {
        self.target(game, place).and_then(Target::as_player)
    }

    /// The answer to the question `d`, which [`crate::choices`] made of a prompt (§7.3). A
    /// question with one answer is answered silently, except under [`Stops::Forced`].
    /// Otherwise the choice record answers it, or an inline source does, or the resolution
    /// pauses here. Once the game is over, as when a player won it mid-resolution, it asks
    /// nothing more.
    pub(crate) fn answer(
        &mut self,
        game: &mut Game<'_, L>,
        d: &Decision<L>,
    ) -> Result<Choice, Stop> {
        debug_assert!(
            self.ability.asks(),
            "a body asked a question without declaring that it pauses"
        );
        debug_assert!(!d.is_empty(), "a question with no answer");
        if game.state.outcome().is_some() {
            return Err(Stop::Pause);
        }
        if d.len() == 1 && game.state.profile().stops != Stops::Forced {
            return Ok(Choice(0));
        }
        if let Some(&c) = self.record.get(self.used) {
            self.used += 1;
            return Ok(Choice(c.into()));
        }
        if let Some(answers) = self.answers.as_deref_mut()
            && answers.inline()
        {
            // An inline answer is a decision like any other, so it counts toward the budget
            // as a paused question would when `next` returns it.
            if game.state.budgets().decisions >= game.state.profile().limits.decisions_per_turn {
                game.state
                    .end_game(Outcome::Stalled(StallReason::DecisionsPerTurn));
                return Err(Stop::Pause);
            }
            game.state.count_decision();
            let answer = answers.answer(Observation::new(game.state, d.player), d);
            debug_assert!(
                answer.is_some(),
                "an inline answer source declined to answer"
            );
            if let Some(c) = answer {
                return Ok(c);
            }
        }
        self.question = Some(d.clone());
        Err(Stop::Pause)
    }
}
