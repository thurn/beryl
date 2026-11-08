//! Starting the game: deck slots, who plays first, and the London mulligan (CR 103).

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Table};
use crate::profile::Profile;
use crate::rng::Rng;
use crate::state::{
    Budgets, DECK_SLOTS, Derived, Flags, ManaPool, MulliganStatus, Object, Pending, Player, State,
    Turn, TurnStep, Zone, Zones, deck_slot,
};
use crate::table::GameTable;
use crate::{turn, zone};

/// The starting hand size ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)),
/// "normally seven".
const STARTING_HAND: u8 = 7;

/// A player before the game begins ([CR 103.4](https://yawgatog.com/resources/magic-rules/#R1034)):
/// "Each player begins the game with a starting life total of 20."
fn new_player<L: Layout>() -> Player<L> {
    Player {
        life: 20,
        mana: ManaPool::default(),
        lands_played: 0,
        drew_from_empty: false,
        mulligans: 0,
        mulligan: MulliganStatus::Undeclared,
        to_bottom: 0,
        emblems: Default::default(),
        sorceries_as_flash: false,
    }
}

impl State {
    /// A new game between the two decks of `t`, determined entirely by `seed` and the choices
    /// that follow. The first decision is the coin flip winner's choice to play or draw.
    ///
    /// Each deck's cards are dealt into its fixed slots by a permutation drawn from the hidden
    /// shuffle stream (architecture §4.2), so a slot number says nothing about the card in
    /// it. That permutation is also the shuffle of
    /// [CR 103.3](https://yawgatog.com/resources/magic-rules/#R1033): "each player shuffles
    /// their deck so that the cards are in a random order."
    pub fn new(t: &GameTable, seed: u64, profile: Profile) -> State {
        let mut s: State = State::empty(profile);
        s.rng = Rng::new(seed);
        for player in Seat::BOTH {
            let mut cards: Fixed<CardIx, DECK_SLOTS> = Fixed::new();
            for &card in t.deck(player) {
                // GameTable::new refuses decks larger than DECK_SLOTS.
                let _ = cards.push(card);
            }
            s.rng.shuffle.shuffle(cards.as_mut_slice());
            for (i, &card) in cards.iter().enumerate() {
                let slot = deck_slot(player, i);
                *s.object_mut(slot) = Object {
                    card,
                    zone: Zone::Library,
                    owner: player,
                    base_controller: player,
                    ..Object::EMPTY
                };
                let _ = s.zones.library[player.index()].push(slot);
                s.zones.members[player.index()][Zone::Library as usize].insert(slot);
            }
        }
        // CR 103.1: "the players determine which one of them will choose who takes the first
        // turn ... (flipping a coin, rolling dice, etc.)".
        let chooser = Seat::new(s.rng.other.below(2) as u8);
        s.pending = Pending::PlayOrDraw { chooser };
        s
    }

    /// Renumbers each library's slots last first, before the game begins: each card stays
    /// where [`State::new`] put it in its library, and the random streams are untouched, so the
    /// game plays out the same under other slot numbers. Slot numbers are public handles that
    /// say nothing about a card (architecture §13), so a save must replay the same either way
    /// (§14). For tests.
    ///
    /// # Panics
    ///
    /// If the game has begun: the choice to play or draw is not open.
    #[doc(hidden)]
    pub fn reverse_deck_slots(&mut self) {
        assert!(
            matches!(self.pending, Pending::PlayOrDraw { .. }) && self.turn.number == 0,
            "the game has not begun"
        );
        for player in Seat::BOTH {
            let p = player.index();
            let slots = self.zones.library[p];
            let mut cards: Fixed<CardIx, DECK_SLOTS> = Fixed::new();
            for &slot in &slots {
                let _ = cards.push(self.object(slot).card);
            }
            for (&slot, &card) in slots.iter().rev().zip(cards.iter()) {
                self.object_mut(slot).card = card;
            }
            self.zones.library[p].as_mut_slice().reverse();
        }
    }
}

impl<L: Layout> State<L> {
    /// Settles who won the coin flip of [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)
    /// as a record says, rather than as the game's randomness drew it: "the players determine
    /// which one of them will choose who takes the first turn". Only while the winner's choice
    /// to play or draw is open.
    ///
    /// # Panics
    ///
    /// If the choice to play or draw is not open.
    pub fn set_flip_winner(&mut self, chooser: Seat) {
        assert!(
            matches!(self.pending, Pending::PlayOrDraw { .. }),
            "the choice to play or draw is open"
        );
        self.pending = Pending::PlayOrDraw { chooser };
    }

    /// A state with no objects, before the game begins.
    pub(crate) fn empty(profile: Profile) -> State<L> {
        State {
            objects: Table::filled(Object::EMPTY),
            battlefield: Default::default(),
            derived: Table::filled(Derived::default()),
            listening: 0,
            zones: Zones {
                members: [[Bitboard::EMPTY; Zone::OWNED]; 2],
                stack: Bitboard::EMPTY,
                library: Default::default(),
                graveyard: Default::default(),
                next_timestamp: 0,
            },
            players: [new_player(), new_player()],
            turn: Turn {
                number: 0,
                active: Seat::P0,
                starting_player: Seat::P0,
                step: TurnStep::Untap,
                priority: Seat::P0,
                passes: 0,
                cleanup_check: false,
                attacked: false,
            },
            stack: Default::default(),
            triggers: Default::default(),
            delayed: Default::default(),
            effects: Default::default(),
            permissions: Default::default(),
            entered_x: Default::default(),
            pending: Pending::None,
            knowledge: [Bitboard::EMPTY; 2],
            rng: Rng::new(0),
            profile,
            budgets: Budgets::default(),
            flags: Flags::default(),
            outcome: None,
        }
    }
}

/// Settles the starting player, then deals opening hands.
///
/// [CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031): "The player chosen to take
/// the first turn is the starting player."
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035): "Each player draws a number
/// of cards equal to their starting hand size, which is normally seven."
pub(crate) fn set_starting_player<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    starting: Seat,
) -> Result<(), Full> {
    s.turn.starting_player = starting;
    s.turn.active = starting;
    for player in [starting, starting.opponent()] {
        zone::draw_n(s, sink, player, STARTING_HAND)?;
    }
    s.pending = Pending::Mulligans;
    advance_mulligans(s, t, sink)
}

/// The players in turn order from the starting player.
pub(crate) fn turn_order<L: Layout>(s: &State<L>) -> [Seat; 2] {
    [s.turn.starting_player, s.turn.starting_player.opponent()]
}

/// Records a keep or mulligan declaration. A player who keeps after taking mulligans then
/// puts that many cards on the bottom ([`keep`]).
pub(crate) fn declare<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    mulligan: bool,
) -> Result<(), Full> {
    if mulligan {
        s.player_mut(player).mulligan = MulliganStatus::WillMulligan;
    } else {
        keep(s, player);
    }
    advance_mulligans(s, t, sink)
}

/// Keeps a player's hand, leaving them a card to put on the bottom for each mulligan taken.
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035) has a player who takes a
/// mulligan draw a new hand, "then [put] a number of those cards equal to the number of
/// times that player has taken a mulligan on the bottom of their library in any order", and
/// "Once a player chooses not to take a mulligan, the remaining cards become that player's
/// opening hand." Here the player looks at all seven and declares first, and puts cards on
/// the bottom only once they keep, as Arena and tournament play do. Nothing anyone can see
/// differs: a hand mulliganed again is shuffled away whole, bottomed cards included, and the
/// bottomed cards are hidden either way.
fn keep<L: Layout>(s: &mut State<L>, player: Seat) {
    let hand = zone::hand_size(s, player) as u8;
    let p = s.player_mut(player);
    p.mulligan = MulliganStatus::Kept;
    p.to_bottom = p.mulligans.min(hand);
}

/// Puts one card from a player's hand on the bottom of their library after a mulligan.
pub(crate) fn put_on_bottom<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
) -> Result<(), Full> {
    zone::put_on_bottom(s, sink, slot)?;
    s.player_mut(player).to_bottom -= 1;
    advance_mulligans(s, t, sink)
}

/// Runs the London mulligan until a player must answer or every player has kept, then begins
/// the first turn.
///
/// [CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035): "First, the starting player
/// declares whether they will take a mulligan. Then each other player in turn order does the
/// same. Once each player has made a declaration, all players who decided to take mulligans do
/// so at the same time. To take a mulligan, a player shuffles the cards in their hand back
/// into their library, draws a new hand of cards equal to their starting hand size, then puts
/// a number of those cards equal to the number of times that player has taken a mulligan on
/// the bottom of their library in any order. ... This process is then repeated until no
/// player takes a mulligan. A player can take mulligans until their opening hand would be zero
/// cards, after which they may not take further mulligans."
fn advance_mulligans<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let order = turn_order(s);
    loop {
        for &p in &order {
            let player = s.player(p);
            if player.mulligan == MulliganStatus::Undeclared && player.mulligans >= STARTING_HAND {
                keep(s, p);
            }
        }
        // Bottom picks come first: they finish the hand just kept.
        if order.iter().any(|&p| s.player(p).to_bottom > 0) {
            return Ok(());
        }
        if order
            .iter()
            .any(|&p| s.player(p).mulligan == MulliganStatus::Undeclared)
        {
            return Ok(());
        }
        let mut any = false;
        for &p in &order {
            if s.player(p).mulligan != MulliganStatus::WillMulligan {
                continue;
            }
            any = true;
            for slot in turn::hand(s, p).iter() {
                zone::move_to(s, sink, slot, Zone::Library)?;
            }
            zone::shuffle_library(s, p);
            zone::draw_n(s, sink, p, STARTING_HAND)?;
            let player = s.player_mut(p);
            player.mulligans += 1;
            player.mulligan = MulliganStatus::Undeclared;
        }
        if !any {
            s.pending = Pending::None;
            return turn::begin_turn(s, t, sink);
        }
    }
}

/// Who answers the open mulligan question, and whether it is a bottom pick.
pub(crate) fn mulligan_question<L: Layout>(s: &State<L>) -> (Seat, bool) {
    let order = turn_order(s);
    if let Some(&p) = order.iter().find(|&&p| s.player(p).to_bottom > 0) {
        return (p, true);
    }
    let p = order
        .into_iter()
        .find(|&p| s.player(p).mulligan == MulliganStatus::Undeclared)
        .unwrap_or(order[0]);
    (p, false)
}
