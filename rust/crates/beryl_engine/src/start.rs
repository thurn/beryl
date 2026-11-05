//! Starting the game: deck slots, who plays first, and the London mulligan (CR 103).

use crate::bitboard::Bitboard;
use crate::event::Sink;
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, Slot};
use crate::profile::Profile;
use crate::rng::Rng;
use crate::state::{
    BATTLEFIELD, Budgets, DECK_SLOTS, Derived, Flags, ManaPool, MulliganStatus, Object, Pending,
    Player, SLOTS, State, Turn, TurnStep, Zone, Zones, deck_slot,
};
use crate::table::GameTable;
use crate::{turn, zone};

/// The starting hand size ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)),
/// "normally seven".
const STARTING_HAND: u8 = 7;

/// A player before the game begins ([CR 103.4](https://yawgatog.com/resources/magic-rules/#R1034)):
/// "Each player begins the game with a starting life total of 20."
fn new_player() -> Player {
    Player {
        life: 20,
        mana: ManaPool::default(),
        lands_played: 0,
        drew_from_empty: false,
        mulligans: 0,
        mulligan: MulliganStatus::Undeclared,
        to_bottom: 0,
        emblems: Fixed::new(),
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
        let mut s = State::empty(profile);
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

    /// A state with no objects, before the game begins.
    pub(crate) fn empty(profile: Profile) -> State {
        State {
            objects: [Object::EMPTY; SLOTS],
            battlefield: Fixed::new(),
            derived: [Derived::default(); BATTLEFIELD],
            zones: Zones {
                members: [[Bitboard::EMPTY; Zone::OWNED]; 2],
                stack: Bitboard::EMPTY,
                library: [Fixed::new(); 2],
                graveyard: [Fixed::new(); 2],
                next_timestamp: 0,
            },
            players: [new_player(); 2],
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
            stack: Fixed::new(),
            triggers: Fixed::new(),
            delayed: Fixed::new(),
            effects: Fixed::new(),
            permissions: Fixed::new(),
            entered_x: Fixed::new(),
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
pub(crate) fn set_starting_player(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    starting: Seat,
) -> Result<(), Full> {
    s.turn.starting_player = starting;
    s.turn.active = starting;
    for player in [starting, starting.opponent()] {
        zone::draw_n(s, player, STARTING_HAND)?;
    }
    s.pending = Pending::Mulligans;
    advance_mulligans(s, t, sink)
}

/// The players in turn order from the starting player.
pub(crate) fn turn_order(s: &State) -> [Seat; 2] {
    [s.turn.starting_player, s.turn.starting_player.opponent()]
}

/// Records a keep or mulligan declaration.
pub(crate) fn declare(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    mulligan: bool,
) -> Result<(), Full> {
    s.player_mut(player).mulligan = if mulligan {
        MulliganStatus::WillMulligan
    } else {
        MulliganStatus::Kept
    };
    advance_mulligans(s, t, sink)
}

/// Puts one card from a player's hand on the bottom of their library after a mulligan.
pub(crate) fn put_on_bottom(
    s: &mut State,
    t: &GameTable,
    sink: &mut Sink,
    player: Seat,
    slot: Slot,
) -> Result<(), Full> {
    zone::put_on_bottom(s, slot)?;
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
fn advance_mulligans(s: &mut State, t: &GameTable, sink: &mut Sink) -> Result<(), Full> {
    let order = turn_order(s);
    loop {
        for &p in &order {
            let player = s.player_mut(p);
            if player.mulligan == MulliganStatus::Undeclared && player.mulligans >= STARTING_HAND {
                player.mulligan = MulliganStatus::Kept;
            }
        }
        // Bottom picks come first: they finish the mulligan just taken.
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
                zone::move_to(s, slot, Zone::Library)?;
            }
            zone::shuffle_library(s, p);
            zone::draw_n(s, p, STARTING_HAND)?;
            let hand = zone::hand_size(s, p) as u8;
            let player = s.player_mut(p);
            player.mulligans += 1;
            player.to_bottom = player.mulligans.min(hand);
            player.mulligan = MulliganStatus::Undeclared;
        }
        if !any {
            s.pending = Pending::None;
            return turn::begin_turn(s, t, sink);
        }
    }
}

/// Who answers the open mulligan question, and whether it is a bottom pick.
pub(crate) fn mulligan_question(s: &State) -> (Seat, bool) {
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
