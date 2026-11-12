//! Mana in a script (plan K23; `manaMotes` in `ManaFlow.tsx`): a streak from what made it
//! into the spell or ability it pays for, which changes no table, and a count of the pool for
//! what is left there or spent from it. What each unit pays for is the engine's word: the
//! entry its mana ability was activated to pay for (`hear.rs`).
//!
//! UI Conventions, "Mana into what it pays for": "Each tapped land glows and sends one streak
//! of its color straight to the spell on the stack ... Mana does not stop at a pool" (Arena,
//! `TbrYIs2juCw 33:32.45`).

use super::build::Builder;
use super::hear::Cause;
use super::{Act, Stream, Subject, Tally};
use crate::protocol::{Mana, ManaPool, Side};

const COLORS: [Mana; 6] = [
    Mana::White,
    Mana::Blue,
    Mana::Black,
    Mana::Red,
    Mana::Green,
    Mana::Colorless,
];

fn amount(pool: &mut ManaPool, color: Mana) -> &mut u8 {
    match color {
        Mana::White => &mut pool.white,
        Mana::Blue => &mut pool.blue,
        Mana::Black => &mut pool.black,
        Mana::Red => &mut pool.red,
        Mana::Green => &mut pool.green,
        Mana::Colorless => &mut pool.colorless,
    }
}

impl Builder<'_> {
    /// Mana is made: it streaks from `source` into what it pays for, or counts in its
    /// player's pool. It pays for the spell or ability its mana ability was activated in the
    /// payment of
    /// ([CR 605.3a](https://yawgatog.com/resources/magic-rules/#R6053a): "A player may
    /// activate an activated mana ability whenever they have priority, whenever they are
    /// casting a spell or activating an ability that requires a mana payment, or whenever a
    /// rule or effect asks for a mana payment"), if the table shows that; mana an effect adds,
    /// or a mana ability activated with nothing to pay, stays in the pool.
    pub(super) fn mana(
        &mut self,
        said: Cause,
        (player, source): (Side, Option<u16>),
        color: Mana,
        amount: u8,
    ) {
        let cause = said.subject();
        // A source sacrificed for its mana has no place left on the table: its streak leaves
        // where it lay while it is going from there in the open beat, and comes from nowhere
        // otherwise.
        let from = source
            .filter(|slot| self.table.objects.contains_key(slot) || self.leaving(*slot))
            .map(|slot| Subject::Object { slot });
        let to = match said {
            Cause::Object { entry, .. } => entry.filter(|&entry| self.stacked(entry)),
            _ => None,
        };
        match to {
            Some(to) => self.streak(from, to, color, amount, cause),
            None => self.pool(player, color, amount, cause),
        }
    }

    /// Whether the open beat takes the object in `slot` from sight where it lay.
    fn leaving(&self, slot: u16) -> bool {
        let gone = Subject::Object { slot };
        self.open
            && self.beats.last().is_some_and(|beat| {
                beat.iter()
                    .any(|step| matches!(step.act, Act::Vanish { subject, .. } if subject == gone))
            })
    }

    fn streak(
        &mut self,
        from: Option<Subject>,
        to: Subject,
        color: Mana,
        amount: u8,
        cause: Option<Subject>,
    ) {
        let stream = Stream::Mana { color };
        self.emit(
            Act::Streak {
                from,
                to,
                stream,
                amount,
                object: None,
            },
            cause,
        );
    }

    /// Mana nothing in the script is paid with counts in its player's pool, as far as the
    /// view's end still has it there.
    fn pool(&mut self, player: Side, color: Mana, made: u8, cause: Option<Subject>) {
        let mut end = self.after.piles(player).mana;
        let mut mana = self.table.piles(player).mana;
        let (now, end) = (amount(&mut mana, color), *amount(&mut end, color));
        let counted = now.saturating_add(made).min(end.max(*now));
        if counted != *now {
            *now = counted;
            let tally = Tally::Pool { player, mana };
            self.emit(Act::Count { tally }, cause);
        }
    }

    /// What `player`'s pool has lost by the view's end went into `to`: a streak of each
    /// color from the player, and the pool's count.
    pub(super) fn drain(&mut self, player: Side, to: Subject, cause: Option<Subject>) {
        let mut end = self.after.piles(player).mana;
        let mut mana = self.table.piles(player).mana;
        let from = Some(Subject::Player { player });
        let mut spent = false;
        for color in COLORS {
            let (now, end) = (amount(&mut mana, color), *amount(&mut end, color));
            if let Some(paid) = now.checked_sub(end).filter(|&paid| paid > 0) {
                *now = end;
                spent = true;
                self.streak(from, to, color, paid, cause);
            }
        }
        if spent {
            let tally = Tally::Pool { player, mana };
            self.emit(Act::Count { tally }, cause);
        }
    }
}
