//! [`Resolution`]: the spell or ability that is resolving, and how it asks its players
//! something (the crate's contract, "Resolution"; architecture §7.3).

use crate::bitboard::Bitboard;
use crate::card::{Ability, CastingIx, ManaColor, Part, Prompt, Stop};
use crate::cast;
use crate::decision::StallReason;
use crate::decision::{Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome};
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
use crate::profile::Stops;
use crate::state::{ObjectRef, StackEntry, Target, TargetPlace, TargetPlaces, TriggerInfo};

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

    /// Asks a question (§7.3). A question with one answer is answered silently, except under
    /// [`Stops::Forced`]. Otherwise the choice record answers it, or an inline source does, or
    /// the resolution pauses here. Once the game is over, as when a player won it
    /// mid-resolution, it asks nothing more.
    fn ask(&mut self, game: &mut Game<'_, L>, d: &Decision<L>) -> Result<Choice, Stop> {
        debug_assert!(
            self.ability.asks(),
            "a body asked a question without declaring may_pause"
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

    /// Asks `player` to pick one of `opts`, as a pick of `prompt` about the objects `about`.
    pub(crate) fn ask_options(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        kind: DecisionKind,
        opts: &[Opt<L>],
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<Opt<L>, Stop> {
        let mut list: L::Queue<Opt<L>, 64> = Default::default();
        for &o in opts {
            list.push(o)?;
        }
        let d = Decision {
            player,
            kind,
            candidates: Candidates::Options(list),
            prompt,
            about,
        };
        let c = self.ask(game, &d)?;
        Ok(d.candidates.option(c).expect("an answer among the options"))
    }

    /// Asks `player` yes or no, as `prompt` says, about the objects `about`: the cards the
    /// question concerns, or what a yes goes on to pick among ([`beryl_card::prompt`], "The
    /// object set").
    pub fn choose_yes_no(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<bool, Stop> {
        let yes_no = &[Opt::Yes, Opt::No];
        let kind = DecisionKind::YesNo;
        Ok(self.ask_options(game, player, kind, yes_no, prompt, about)? == Opt::Yes)
    }

    /// Asks `player` to choose up to `n` of `candidates`, one at a time, each answer either one
    /// more object or [`Opt::Done`], as picks of `prompt`. Interchangeable candidates are one
    /// answer (§7.2).
    pub fn choose_up_to(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        n: u8,
        prompt: Prompt,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        let mut chosen = Bitboard::EMPTY;
        while chosen.count() < u32::from(n) && !(candidates & !chosen).is_empty() {
            match self.choose_or_done(game, player, candidates & !chosen, prompt)? {
                Some(slot) => chosen.insert(slot),
                None => break,
            }
        }
        Ok(chosen)
    }

    /// Asks `player` for one more of `candidates`, as an [`Opt::Object`], or [`Opt::Done`].
    /// Interchangeable candidates are one answer (§7.2).
    pub fn choose_or_done(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<Option<L::Slot>, Stop> {
        let mut opts: L::Queue<Opt<L>, 64> = Default::default();
        let left = crate::engine::collapse(game.state, candidates);
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        let left = game.state.in_candidate_order(left).map(Opt::Object);
        for o in core::iter::once(Opt::Done).chain(left) {
            opts.push(o)?;
        }
        let kind = DecisionKind::ChooseUpTo;
        let none = Bitboard::EMPTY;
        Ok(
            match self.ask_options(game, player, kind, opts.as_slice(), prompt, none)? {
                Opt::Object(slot) => Some(slot),
                _ => None,
            },
        )
    }

    /// Asks `player` to choose one of `candidates`, which must not be empty, as a pick of
    /// `prompt`.
    pub fn choose_object(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<L::Slot, Stop> {
        let kind = DecisionKind::ChooseObject;
        self.ask_object(game, player, kind, candidates, prompt, Bitboard::EMPTY)
    }

    /// Asks `player` for one of `candidates` as a question of `kind`, a pick of `prompt` about
    /// the objects `about`.
    pub(crate) fn ask_object(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        kind: DecisionKind,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<L::Slot, Stop> {
        let d = Decision {
            player,
            kind,
            candidates: Candidates::Objects(crate::engine::collapse(game.state, candidates)),
            prompt,
            about,
        };
        let c = self.ask(game, &d)?;
        Ok(d.candidates.object(c).expect("an answer among the objects"))
    }

    /// Asks `player` to choose one of `candidates`, or none when they may decline: a yes or
    /// no first ([`Part::Whether`]), then the object, both picks of `prompt`. Both carry the
    /// candidates as the prompt's object set: the yes or no does not offer them, and the
    /// pick offers one of each group of interchangeable ones (§7.2). With no candidates there
    /// is nothing to ask.
    pub fn may_choose_object(
        &mut self,
        game: &mut Game<'_, L>,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<Option<L::Slot>, Stop> {
        if candidates.is_empty()
            || !self.choose_yes_no(game, player, prompt.part(Part::Whether), candidates)?
        {
            return Ok(None);
        }
        let kind = DecisionKind::ChooseObject;
        let pick = prompt.part(Part::Pick);
        self.ask_object(game, player, kind, candidates, pick, candidates)
            .map(Some)
    }
}
