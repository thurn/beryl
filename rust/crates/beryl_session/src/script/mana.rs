//! Mana in a script (plan K23; `manaMotes` in `ManaFlow.tsx`): a streak from what made it
//! into the spell or ability it pays for, which changes no table, and a count of the pool for
//! what is left there or spent from it. What each unit pays for is `infer.rs`'s guess.
//!
//! UI Conventions, "Mana into what it pays for": "Each tapped land glows and sends one streak
//! of its color straight to the spell on the stack ... Mana does not stop at a pool" (Arena,
//! `TbrYIs2juCw 33:32.45`).

use super::build::Builder;
use super::infer::{self, Pays, Told};
use super::table::Entry;
use super::{Act, Stream, Subject, Tally};
use crate::protocol::{GameEvent, Mana, ManaPool, Side, StackItem};

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

/// Mana made before the ability it pays for is on the stack: its streak waits for the event
/// that announces the ability.
pub(super) struct Owed {
    /// The index of that event.
    event: usize,
    from: Option<Subject>,
    color: Mana,
    amount: u8,
    cause: Option<Subject>,
}

impl Builder<'_> {
    /// Mana is made: it streaks from `source` into what it pays for, or counts in its
    /// player's pool.
    pub(super) fn mana(
        &mut self,
        told: Told,
        (player, source): (Side, Option<u16>),
        color: Mana,
        amount: u8,
        cause: Option<Subject>,
    ) {
        // A source sacrificed for its mana may have no place left on the table.
        let from = source
            .filter(|slot| self.table.objects.contains_key(slot))
            .map(|slot| Subject::Object { slot });
        let events = self.events;
        let to = match told.pays {
            Pays::Pool => None,
            Pays::Spell(slot) => self.on_stack(Subject::Object { slot }),
            Pays::Top => self.table.stack.last().map(Entry::subject),
            Pays::Event(event) => match &events[event] {
                GameEvent::ResolutionPayment { item, .. } => self.paid_into(item),
                GameEvent::Ability { item, .. } => {
                    let Some(at) = self.making(item) else {
                        self.owed.push(Owed {
                            event,
                            from,
                            color,
                            amount,
                            cause,
                        });
                        return;
                    };
                    Some(self.table.stack[at].subject())
                }
                _ => None,
            },
        };
        match to {
            Some(to) => self.streak(from, to, color, amount, cause),
            None => self.pool(player, color, amount, cause),
        }
    }

    fn on_stack(&self, subject: Subject) -> Option<Subject> {
        let mut stack = self.table.stack.iter().map(Entry::subject);
        stack.find(|&entry| entry == subject)
    }

    /// The resolving spell or ability `item` a payment names, as the table has it.
    pub(super) fn paid_into(&self, item: &StackItem) -> Option<Subject> {
        let mut stack = self.table.stack.iter().rev();
        stack
            .find(|entry| infer::same(&entry.item, item))
            .map(Entry::subject)
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

    /// The ability event `event` announces is on the stack as `to`: the mana made for it
    /// streaks into it, in one beat.
    pub(super) fn pay_owed(&mut self, event: usize, to: Subject) {
        let (owed, rest) = std::mem::take(&mut self.owed)
            .into_iter()
            .partition(|owed| owed.event == event);
        self.owed = rest;
        let owed: Vec<Owed> = owed;
        if !owed.is_empty() {
            self.open = false;
        }
        for mana in owed {
            self.streak(mana.from, to, mana.color, mana.amount, mana.cause);
        }
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
