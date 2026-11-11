//! How a card body asks its player something (architecture §7.3): each question is one
//! factored pick of a prompt, answered from the choice record, by an inline source, or by
//! pausing the body.

use super::EffectCtx;
use crate::bitboard::Bitboard;
use crate::card::{Part, Pause, Prompt, Purpose};
use crate::decision::{Candidates, Choice, Decision, DecisionKind, Opt, StallReason};
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::observation::Observation;
use crate::profile::Stops;
use crate::state::{ObjectRef, Zone, ZoneKey};

impl<L: Layout> EffectCtx<'_, L> {
    /// Asks a question (§7.3). A question with one answer is answered silently, except under
    /// [`Stops::Forced`]. Otherwise the choice record answers it, or an inline source does, or
    /// the body pauses here. Once the game is over, as when a player won it mid-resolution,
    /// the body asks nothing more.
    fn ask(&mut self, d: &Decision<L>) -> Result<Choice, Pause> {
        debug_assert!(
            self.ability.asks(),
            "a body asked a question without declaring may_pause"
        );
        debug_assert!(!d.is_empty(), "a question with no answer");
        if self.state.outcome().is_some() {
            return Err(Pause);
        }
        if d.len() == 1 && self.state.profile().stops != Stops::Forced {
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
            if self.state.budgets().decisions >= self.state.profile().limits.decisions_per_turn {
                self.stall(StallReason::DecisionsPerTurn);
                return Err(Pause);
            }
            self.state.count_decision();
            let answer = answers.answer(Observation::new(self.state, d.player), d);
            debug_assert!(
                answer.is_some(),
                "an inline answer source declined to answer"
            );
            if let Some(c) = answer {
                return Ok(c);
            }
        }
        self.question = Some(d.clone());
        Err(Pause)
    }

    pub(super) fn ask_options(
        &mut self,
        player: Seat,
        kind: DecisionKind,
        opts: &[Opt<L>],
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<Opt<L>, Pause> {
        let mut list: L::Queue<Opt<L>, 64> = Default::default();
        for &o in opts {
            if list.push(o).is_err() {
                self.full = true;
                return Err(Pause);
            }
        }
        let d = Decision {
            player,
            kind,
            candidates: Candidates::Options(list),
            prompt,
            about,
        };
        let c = self.ask(&d)?;
        Ok(d.candidates.option(c).expect("an answer among the options"))
    }

    /// Asks `player` yes or no, as `prompt` says, about the objects `about`: the cards the
    /// question concerns, or what a yes goes on to pick among ([`beryl_card::prompt`], "The
    /// object set").
    pub fn choose_yes_no(
        &mut self,
        player: Seat,
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<bool, Pause> {
        let yes_no = &[Opt::Yes, Opt::No];
        Ok(self.ask_options(player, DecisionKind::YesNo, yes_no, prompt, about)? == Opt::Yes)
    }

    /// Asks `player` to choose up to `n` of `candidates`, one at a time, each answer either one
    /// more object or [`Opt::Done`], as picks of `prompt`. Interchangeable candidates are one
    /// answer (§7.2).
    pub fn choose_up_to(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        n: u8,
        prompt: Prompt,
    ) -> Result<Bitboard<L::Slot>, Pause> {
        let mut chosen = Bitboard::EMPTY;
        while chosen.count() < u32::from(n) && !(candidates & !chosen).is_empty() {
            match self.choose_or_done(player, candidates & !chosen, prompt)? {
                Some(slot) => chosen.insert(slot),
                None => break,
            }
        }
        Ok(chosen)
    }

    /// Asks `player` for one more of `candidates`, as an [`Opt::Object`], or [`Opt::Done`].
    /// Interchangeable candidates are one answer (§7.2).
    pub(super) fn choose_or_done(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<Option<L::Slot>, Pause> {
        let mut opts: L::Queue<Opt<L>, 64> = Default::default();
        let left = crate::engine::collapse(self.state, candidates);
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        let left = self.state.in_candidate_order(left).map(Opt::Object);
        for o in core::iter::once(Opt::Done).chain(left) {
            if opts.push(o).is_err() {
                self.full = true;
                return Err(Pause);
            }
        }
        let kind = DecisionKind::ChooseUpTo;
        Ok(
            match self.ask_options(player, kind, opts.as_slice(), prompt, Bitboard::EMPTY)? {
                Opt::Object(slot) => Some(slot),
                _ => None,
            },
        )
    }

    /// Asks `player` to choose one of `candidates`, which must not be empty, as a pick of
    /// `prompt`.
    pub fn choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<L::Slot, Pause> {
        let kind = DecisionKind::ChooseObject;
        self.ask_object(player, kind, candidates, prompt, Bitboard::EMPTY)
    }

    fn ask_object(
        &mut self,
        player: Seat,
        kind: DecisionKind,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<L::Slot, Pause> {
        let d = Decision {
            player,
            kind,
            candidates: Candidates::Objects(crate::engine::collapse(self.state, candidates)),
            prompt,
            about,
        };
        let c = self.ask(&d)?;
        Ok(d.candidates.object(c).expect("an answer among the objects"))
    }

    /// `player` discards a card of their choice
    /// ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)): "By default, effects
    /// that cause a player to discard a card allow the affected player to choose which card
    /// to discard." Returns the card in the graveyard, or `None` from an empty hand.
    pub fn discard_a_card(&mut self, player: Seat) -> Result<Option<ObjectRef<L>>, Pause> {
        self.discard_one_of(player, self.state.zone(ZoneKey::Of(player, Zone::Hand)))
    }

    /// `player` discards `n` cards of their choice, or their whole hand if it has fewer
    /// (CR 701.9b), one pick for each: the picks of one prompt, which says at each how many
    /// are still to discard. Returns the cards, in the graveyard.
    pub fn discard_cards(&mut self, player: Seat, n: u8) -> Result<Bitboard<L::Slot>, Pause> {
        let mut discarded = Bitboard::EMPTY;
        for left in (1..=n).rev() {
            let hand = self.state.zone(ZoneKey::Of(player, Zone::Hand));
            let Some(card) = self.discard_of(player, hand, left)? else {
                break;
            };
            discarded.insert(card.slot);
        }
        Ok(discarded)
    }

    /// `player` discards one of `cards`, cards in their hand, of their choice, as Thirst for
    /// Discovery's "discard a basic land card" (CR 701.9b). Returns the card in the
    /// graveyard, or `None` when there is none to discard.
    pub fn discard_one_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
    ) -> Result<Option<ObjectRef<L>>, Pause> {
        self.discard_of(player, cards, 1)
    }

    /// One discard of `left` still to make, among `cards`.
    fn discard_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
        left: u8,
    ) -> Result<Option<ObjectRef<L>>, Pause> {
        debug_assert!((cards & !self.state.zone(ZoneKey::Of(player, Zone::Hand))).is_empty());
        if cards.is_empty() {
            return Ok(None);
        }
        let prompt = Prompt::new(Purpose::Discard).most(left);
        let kind = DecisionKind::Discard;
        let slot = self.ask_object(player, kind, cards, prompt, Bitboard::EMPTY)?;
        self.discard(self.state.object_ref(slot));
        Ok(Some(self.state.object_ref(slot)))
    }

    /// Asks `player` to choose one of `candidates`, or none when they may decline: a yes or
    /// no first ([`Part::Whether`]), then the object, both picks of `prompt`. Both carry the
    /// candidates as the prompt's object set: the yes or no does not offer them, and the
    /// pick offers one of each group of interchangeable ones (§7.2). With no candidates there
    /// is nothing to ask.
    pub fn may_choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<Option<L::Slot>, Pause> {
        if candidates.is_empty()
            || !self.choose_yes_no(player, prompt.part(Part::Whether), candidates)?
        {
            return Ok(None);
        }
        let kind = DecisionKind::ChooseObject;
        self.ask_object(
            player,
            kind,
            candidates,
            prompt.part(Part::Pick),
            candidates,
        )
        .map(Some)
    }
}
