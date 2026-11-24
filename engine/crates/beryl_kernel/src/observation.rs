//! `Observation`: a borrowed view of the state for one player, masking what they do not know
//! (architecture §13, decision D9).
//!
//! Implements: none. Projects the game state using the viewer's recorded knowledge.

mod subgames;

use crate::bitboard::Bitboard;
use crate::filter::{Evaluate, Filter};
use crate::ids::{CardId, Seat, Slot, SlotId};
use crate::layout::{Layout, Queue, Small};
use crate::rng::SplitMix64;
use crate::state::{
    Derived, Flag, Object, Pending, PendingTrigger, Player, StackEntry, State, Turn, Zone, ZoneKey,
};
use crate::subgame::OuterGame;
use crate::table::GameTable;
use beryl_card::{Ability, Behavior, FaceIx, Objects, Words};

/// The state as one player sees it. Slots are public handles; what is hidden is which card
/// sits behind a slot the viewer has not seen. Agents, rollout policies and the UI read the
/// game only through this.
///
/// While a resolution is paused (§7.3) the position shown is not the committed one: the body
/// has run in scratch up to its question (`beryl_engine::view`). The view keeps the committed
/// state as its root, the position search plays on from; `beryl_engine::observe` builds such a
/// view.
#[derive(Clone, Copy)]
pub struct Observation<'a, L: Layout = Small> {
    state: &'a State<L>,
    root: &'a State<L>,
    viewer: Seat,
}

impl<'a, L: Layout> Observation<'a, L> {
    /// The view of a committed state, which is its own root.
    pub fn new(state: &'a State<L>, viewer: Seat) -> Self {
        Self {
            state,
            root: state,
            viewer,
        }
    }

    /// The view of `shown`, the position a paused resolution's question is asked in, whose
    /// committed state is `root`.
    pub fn paused(shown: &'a State<L>, root: &'a State<L>, viewer: Seat) -> Self {
        Self {
            state: shown,
            root,
            viewer,
        }
    }

    /// The position shown, for the engine's own reads of public facts about it.
    pub fn shown(&self) -> &'a State<L> {
        self.state
    }

    pub fn viewer(&self) -> Seat {
        self.viewer
    }

    pub fn knows(&self, slot: L::Slot) -> bool {
        self.state.knows(self.viewer, slot)
    }

    /// The game index of the card in a slot, if the viewer knows it.
    pub fn card(&self, slot: L::Slot) -> Option<L::Card> {
        self.knows(slot).then(|| self.state.card(slot))
    }

    /// The card a permanent under a copy effect really is ([`State::really`]), if one does
    /// and the viewer knows it: not a face-down component under a merged permanent's face-up
    /// top that the viewer never saw ([`crate::Memo::FaceDownUnder`]).
    pub fn really(&self, slot: L::Slot) -> Option<L::Card> {
        let card = self.state.really(slot)?;
        self.state
            .knows_card_really_is(self.viewer, slot)
            .then_some(card)
    }

    /// The copiable values of the object in a slot ([`State::copiable`]), if the viewer
    /// knows its card. A merged permanent's other cards are as public as it is
    /// ([`crate::query::Copiable::merged_faces`]).
    pub fn copiable(&self, slot: L::Slot) -> Option<crate::query::Copiable<L>> {
        self.knows(slot).then(|| self.state.copiable(slot))
    }

    /// The object in a slot, with every field that could betray an unknown identity masked:
    /// its card, which only [`Observation::card`] reads, and the face up. Whether a
    /// permanent is face down is public: it is part of its status
    /// (see [CR 110.5](https://yawgatog.com/resources/magic-rules/#R1105)).
    pub fn object(&self, slot: L::Slot) -> Object<L> {
        let mut o = self.state.object(slot).clone();
        if !self.knows(slot) {
            o.card = L::Card::FIRST;
            if o.face != FaceIx::DOWN {
                o.face = FaceIx::FRONT;
            }
        }
        o
    }

    /// The other face of a known card or token whose face is up. This uses the observed
    /// identity so a hidden card's second face cannot reveal what the card is.
    pub fn other_face_name(&self, t: &GameTable, slot: L::Slot) -> Option<&'static str> {
        let card = self.card(slot)?;
        t.def(card).other_face_name(self.state.object(slot).face)
    }

    /// Who controls an object: for a permanent the layer-2 result, otherwise its base
    /// controller (see [CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)). Public:
    /// control is never hidden.
    pub fn controller(&self, slot: L::Slot) -> Seat {
        self.state.controller(slot)
    }

    pub fn player(&self, player: Seat) -> &'a Player<L> {
        self.state.player(player)
    }

    pub fn turn(&self) -> &'a Turn {
        self.state.turn()
    }

    /// The player chosen to take the first turn; none while the coin flip's winner chooses.
    /// See [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031): "the players determine
    /// which one of them will choose who takes the first turn ... The player chosen to take
    /// the first turn is the starting player."
    pub fn starting_player(&self) -> Option<Seat> {
        match self.state.pending() {
            Pending::PlayOrDraw { .. } => None,
            Pending::None
            | Pending::Mulligans
            | Pending::Discard
            | Pending::LegendRule { .. }
            | Pending::EnterPayingLife { .. }
            | Pending::DeclareAttackers { .. }
            | Pending::DeclareBlockers { .. }
            | Pending::CombatDamage { .. }
            | Pending::ExactCombatDamage(_)
            | Pending::Casting(_)
            | Pending::Resolving(_)
            | Pending::StepActions(_)
            | Pending::TurnStart(_)
            | Pending::TurningFaceUp { .. }
            | Pending::OrderEntered { .. }
            | Pending::EnterChoosing { .. }
            | Pending::ManaColor { .. } => Some(self.state.turn().starting_player),
        }
    }

    /// The objects in the zone `key` names: public, though the identities may not be.
    pub fn zone(&self, key: ZoneKey) -> Bitboard<L::Slot> {
        self.state.zone(key)
    }

    /// A player's library as slots, bottom first.
    pub fn library(&self, player: Seat) -> &'a [L::Slot] {
        self.state.library(player)
    }

    /// A player's graveyard as slots, bottom first.
    pub fn graveyard(&self, player: Seat) -> &'a [L::Slot] {
        self.state.graveyard(player)
    }

    pub fn battlefield(&self) -> &'a [L::Slot] {
        self.state.battlefield()
    }

    /// A permanent's characteristics after continuous effects, public as the battlefield is
    /// (see [CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)).
    ///
    /// # Panics
    ///
    /// If `slot` is not on the battlefield.
    pub fn derived(&self, slot: L::Slot) -> &'a Derived<L> {
        self.state.derived(slot)
    }

    /// The stack, bottom first. This is a public zone (see CR 400.2). A face-down
    /// spell records its public characteristics, and its card is visible through
    /// [`Self::card`] to its controller (see CR 708.5).
    pub fn stack(&self) -> &'a [StackEntry<L>] {
        self.state.stack()
    }

    /// The triggered abilities waiting to be put on the stack. Each names its public source;
    /// [`crate::decision::DecisionKind::TriggerOrder`] options index this list.
    pub fn triggers(&self) -> &'a [PendingTrigger<L>] {
        self.state.triggers()
    }

    /// Whether the object in `slot` passes `filter`, evaluated for the viewer through `words`,
    /// the text changes of the object the filter is printed on ([`Self::words_of`]).
    /// Face-down objects have publicly known characteristics. Other unknown cards have no
    /// visible characteristics to match.
    pub fn matches(&self, t: &GameTable, filter: &Filter, slot: L::Slot, words: &Words) -> bool {
        (self.knows(slot) || self.state.object(slot).face == FaceIx::DOWN)
            && filter.matches(self.state, t, slot, self.viewer, words)
    }

    /// The text changes on the object in `slot` ([`State::words_of`]), which every player
    /// sees: a text-changing effect is public.
    pub fn words_of(&self, slot: L::Slot) -> Words {
        self.state.words_of(slot)
    }

    /// The committed state behind the view, unmasked. Reading it is cheating: only the
    /// omniscient search, which is labeled as cheating wherever a human meets it, does
    /// (architecture §19.1). The information-independence test holds every other agent to the
    /// masked accessors.
    pub fn unmasked_root(&self) -> &'a State<L> {
        self.root
    }

    /// A full committed state consistent with everything the viewer knows there: the root,
    /// with the unknown remainder of each decklist dealt at random into that player's slots
    /// the viewer does not know (architecture §13). Decklists are public (decision D19), so
    /// the remainder is exact. The cards the viewer knows, every public fact and the slot
    /// order of every zone are kept; the game's random streams are reseeded from `rng`, so its
    /// seed does not leak. A paused resolution keeps what it showed the viewer known in the
    /// committed state, so it replays from here to the same question.
    pub fn determinize(&self, t: &GameTable, rng: &mut SplitMix64) -> State<L> {
        self.deal(t, rng, false)
    }

    /// [`Self::determinize`] with every hand shown: only the libraries' unknown cards, and
    /// any other card the viewer does not know outside a hand, are dealt at random. This is
    /// half-cheating, for measuring what seeing the opponent's hand is worth: a hand is a
    /// hidden zone (see [CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player
    /// may arrange their hand in any convenient fashion and look at it at any time. A
    /// player can't look at the cards in another player's hand but may count those cards
    /// at any time."), and so is a library, whose order no player
    /// knows (see [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Each library
    /// must be kept in a single face-down pile. Players can't look at or change the order of
    /// cards in a library.").
    pub fn determinize_libraries(&self, t: &GameTable, rng: &mut SplitMix64) -> State<L> {
        self.deal(t, rng, true)
    }

    /// The root with each player's cards the viewer does not know, `shown` ones aside, dealt
    /// at random from the rest of their decklist.
    ///
    /// A card a player picked without revealing it, earlier in the paused resolution
    /// ([`crate::pending::Resolving`], "Hidden picks"), is dealt one the pick's filter
    /// accepts: the viewer knows the slot was picked, and so that it held such a card
    /// (see [CR 101.4a]): "each player must clearly indicate which face-down card they are
    /// choosing." Where the deal put another card there, it trades with the first other slot
    /// of that player's unknown cards, in slot order, whose card passes; the deal's order is
    /// random, so which card that is is too.
    ///
    /// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
    fn deal(&self, t: &GameTable, rng: &mut SplitMix64, show_hands: bool) -> State<L> {
        let mut s = self.root.clone();
        if !s.in_subgame() || !self.redeal_subgames(t, rng, show_hands, &mut s) {
            self.redeal(t, rng, show_hands, &mut s);
        }
        s
    }

    fn redeal(&self, t: &GameTable, rng: &mut SplitMix64, show_hands: bool, s: &mut State<L>) {
        let picks = self.unseen_hidden_picks(t);
        for player in Seat::BOTH {
            const IN_GAME: usize =
                Slot::DECK_SLOTS + crate::format::Format::CONSTRUCTED.maximum_sideboard();
            let mut remainder: L::Queue<L::Card, IN_GAME> = Default::default();
            let slots = || self.root.in_game_card_slots(t, player);
            if self.root.in_subgame() {
                // This deck contains only cards carried into the subgame.
                for slot in slots() {
                    remainder.push_bounded(self.root.card_really_is(slot));
                }
            } else {
                // The sideboard list and every card revealed as it came in are public
                // (architecture decision D19). Only cards in the game are redealt.
                let admitted = self.root.admitted(t, player).map(|(_, card)| card);
                for card in t.deck(player).chain(admitted) {
                    // The queue holds a small game's main deck and all its sideboard cards.
                    remainder.push_bounded(card);
                }
            }
            let mut unknown: L::Queue<L::Slot, IN_GAME> = Default::default();
            // A copy can hide the card underneath while its copy effect stays in force.
            // Deal that card into its `Memo::Really`, separately from the slots that
            // can trade their card fields to satisfy a hidden pick.
            let mut hidden_originals: L::Queue<L::Slot, IN_GAME> = Default::default();
            for slot in slots() {
                if (show_hands && self.root.object(slot).zone == Zone::Hand)
                    || self.root.knows_card_really_is(self.viewer, slot)
                {
                    let card = s.card_really_is(slot);
                    let at = remainder
                        .position(card)
                        .expect("a known card not in its deck or among the admitted cards");
                    remainder.remove(at);
                } else if self.root.object(slot).flags.has(Flag::Copy) {
                    hidden_originals.push_bounded(slot);
                } else {
                    unknown.push_bounded(slot);
                }
            }
            rng.shuffle(remainder.as_mut_slice());
            // Casting face down is public evidence of the card's morph ability.
            // Choose those unknown cards from the shuffled remainder before filling the
            // unconstrained slots. No assignment reads the card previously in a slot.
            for (position, &slot) in unknown.iter().enumerate() {
                if !s.requires_morph(slot) {
                    continue;
                }
                let Some(index) = (0..remainder.len()).find(|&i| {
                    if i < position && s.requires_morph(unknown.as_slice()[i]) {
                        return false;
                    }
                    let def = t.def(remainder.as_slice()[i]);
                    def.front()
                        .casting
                        .iter()
                        .any(|option| matches!(option, beryl_card::CastingOption::Morph(_)))
                }) else {
                    break;
                };
                remainder.as_mut_slice().swap(position, index);
            }
            for (&slot, &card) in unknown.iter().zip(remainder.iter()) {
                s.object_mut(slot).card = card;
            }
            let dealt = remainder.iter().skip(unknown.len());
            for (&slot, &card) in hidden_originals.iter().zip(dealt) {
                s.deal_card_really_is(slot, card);
            }
            if let Some(picks) = &picks {
                picks.deal(s, t, unknown.as_slice());
            }
        }
        s.reseed(rng.next_u64());
        if let (Some(outer), Some(dealt)) = (self.root.outer.get(), s.outer.get_mut()) {
            Observation::new(outer, self.viewer).redeal(t, rng, show_hands, dealt);
        }
    }

    /// The slots the paused resolution's hidden picks have picked so far, known card or not
    /// ([`crate::state::Resolving::hidden_picks`]). Which slot was picked is public and the
    /// card is not (see [CR 101.4a]): "those cards may remain face down as they're chosen.
    /// However, each player must clearly indicate which face-down card they are choosing."
    ///
    /// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
    pub fn hidden_picks(&self) -> Bitboard<L::Slot> {
        let mut slots = Bitboard::EMPTY;
        if let Pending::Resolving(r) = self.root.pending() {
            for slot in r.hidden_picks() {
                slots.insert(slot);
            }
        }
        slots
    }

    /// The hidden picks of the paused resolution whose cards the viewer does not know, and
    /// the filter each passed ([`HiddenPicks`]); none where there are none.
    fn unseen_hidden_picks(&self, t: &GameTable) -> Option<HiddenPicks<L>> {
        let Pending::Resolving(r) = self.root.pending() else {
            return None;
        };
        let mut slots = Bitboard::EMPTY;
        for slot in r.hidden_picks() {
            if !self.root.knows(self.viewer, slot) {
                slots.insert(slot);
            }
        }
        if slots.is_empty() {
            return None;
        }
        // A hidden pick is among the cards of a hand or library each player has, which
        // pass a filter read for the controller of what is resolving (`Malformed::Chosen`).
        let entry = self.root.stack().last()?;
        let ability = entry.ability_in(t).map(Ability::body);
        let Some(Behavior::Data(effect)) = ability else {
            debug_assert!(false, "a hidden pick recorded by a Rust body");
            return None;
        };
        let Some(Objects::InZoneOf { filter, .. }) = effect.hidden_pick() else {
            debug_assert!(false, "a hidden pick recorded by an ability that has none");
            return None;
        };
        Some(HiddenPicks {
            slots,
            filter,
            you: entry.controller(),
            words: self.root.entry_words(entry),
        })
    }
}

/// The slots hidden picks picked that a viewer does not know, and the filter each card
/// picked passed, read for `you` through `words`.
struct HiddenPicks<L: Layout> {
    slots: Bitboard<L::Slot>,
    filter: Filter,
    you: Seat,
    words: Words,
}

impl<L: Layout> HiddenPicks<L> {
    /// Gives each picked slot among `unknown`, the slots of one player dealt at random in
    /// `s`, a card the filter accepts, trading with another of `unknown` that is not picked
    /// ([`Observation::deal`]).
    fn deal(&self, s: &mut State<L>, t: &GameTable, unknown: &[L::Slot]) {
        for &slot in unknown.iter().filter(|&&slot| self.slots.contains(slot)) {
            if self.passes(s, t, slot) {
                continue;
            }
            let dealt = s.object(slot).card;
            let traded = unknown.iter().any(|&other| {
                if self.slots.contains(other) || s.requires_morph(other) {
                    return false;
                }
                let card = s.object(other).card;
                s.object_mut(slot).card = card;
                let passes = self.passes(s, t, slot);
                s.object_mut(slot).card = dealt;
                if passes {
                    s.object_mut(slot).card = card;
                    s.object_mut(other).card = dealt;
                }
                passes
            });
            // The card truly picked is among those dealt, so one passes.
            debug_assert!(traded, "no card the hidden pick's filter accepts");
        }
    }

    fn passes(&self, s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
        self.filter.matches(s, t, slot, self.you, &self.words)
    }
}

impl<L: Layout> State<L> {
    /// The public slot in which a spell was cast for morph (see CR 702.37c).
    fn requires_morph(&self, slot: L::Slot) -> bool {
        self.object(slot).face == FaceIx::DOWN
            && (self.object(slot).zone == Zone::Stack || self.morphed(slot))
    }
}
