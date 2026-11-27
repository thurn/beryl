//! The features `value-test`'s models read: what the deciding player can see of a position,
//! from its `Observation` alone.

use beryl_engine::card::Types;
use beryl_engine::{CardId, GameTable, LargeCardIx, Layout, Observation, Seat, Zone, ZoneKey};

/// How many features describe one player.
const PER_PLAYER: usize = 11;

/// How many features each distinct card of the decks has: its count in the decider's hand,
/// on each player's battlefield and in each graveyard.
const PER_CARD: usize = 5;

/// The columns of a game table's feature vectors.
pub struct Columns {
    /// Each game index's card column, none for a card in neither deck (a token, an emblem).
    card: Vec<Option<usize>>,
    cards: usize,
}

impl Columns {
    /// The columns for `t`: one block per distinct card of the two decks, by game index.
    pub fn new(t: &GameTable) -> Columns {
        let mut card = vec![None; t.len()];
        for seat in Seat::BOTH {
            for c in t.deck::<LargeCardIx>(seat) {
                card[c.index()] = Some(0);
            }
        }
        let mut cards = 0;
        for column in card.iter_mut().flatten() {
            *column = cards;
            cards += 1;
        }
        Columns { card, cards }
    }

    /// How many features there are.
    pub fn len(&self) -> usize {
        2 + 2 * PER_PLAYER + PER_CARD * self.cards
    }

    /// Whether there are none, which never happens.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// The features of the position `obs` shows its viewer, the decider: the turn number,
    /// whether the decider is the active player, eleven counts for the decider and then the
    /// opponent, and the card counts.
    pub fn read<L: Layout>(&self, obs: Observation<'_, L>) -> Vec<f64> {
        let me = obs.viewer();
        // Games here are two-player: the opponent is the seat that is not the decider.
        let them = Seat::BOTH.into_iter().find(|&p| p != me).unwrap_or(me);
        let mut f = vec![0.0; self.len()];
        f[0] = f64::from(obs.turn().number());
        f[1] = f64::from(u8::from(obs.turn().active() == me));
        let cards = 2 + 2 * PER_PLAYER;
        let mut count = |block: usize, card: Option<L::Card>| {
            if let Some(column) = card.and_then(|c| self.card.get(c.index()).copied().flatten()) {
                f[cards + PER_CARD * column + block] += 1.0;
            }
        };
        let hand = obs.zone(ZoneKey::Of(me, Zone::Hand));
        for slot in hand.sorted(|s| s) {
            count(0, obs.card(slot));
        }
        for &slot in obs.battlefield() {
            let block = if obs.controller(slot) == me { 1 } else { 2 };
            count(block, obs.card(slot));
        }
        for (block, p) in [(3, me), (4, them)] {
            for &slot in obs.graveyard(p) {
                count(block, obs.card(slot));
            }
        }
        for (i, p) in [me, them].into_iter().enumerate() {
            let at = 2 + PER_PLAYER * i;
            f[at..at + PER_PLAYER].copy_from_slice(&player(obs, p));
        }
        f
    }
}

/// Player `p`'s eleven counts: life, cards in hand, library size, lands, untapped lands,
/// graveyard size, creatures, their total power and toughness, planeswalkers and their total
/// loyalty.
fn player<L: Layout>(obs: Observation<'_, L>, p: Seat) -> [f64; PER_PLAYER] {
    use beryl_engine::{SignedNumber as _, UnsignedNumber as _};
    let mut f = [0.0; PER_PLAYER];
    f[0] = f64::from(obs.player(p).life().to_i32());
    f[1] = f64::from(obs.zone(ZoneKey::Of(p, Zone::Hand)).count());
    f[2] = obs.library(p).len() as f64;
    f[5] = obs.graveyard(p).len() as f64;
    for &slot in obs.battlefield() {
        if obs.controller(slot) != p {
            continue;
        }
        let d = obs.derived(slot);
        let o = obs.object(slot);
        if d.types.intersects(Types::LAND) {
            f[3] += 1.0;
            f[4] += f64::from(u8::from(!o.is_tapped()));
        }
        if d.types.intersects(Types::CREATURE) {
            f[6] += 1.0;
            f[7] += f64::from(d.power.to_i32());
            f[8] += f64::from(d.toughness.to_i32());
        }
        if d.types.intersects(Types::PLANESWALKER) {
            f[9] += 1.0;
            f[10] += f64::from(o.counters().loyalty.to_u32());
        }
    }
    f
}
