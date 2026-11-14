//! Events, their causes and their tags in another layout: a part of [`super::Relayout`].

use crate::cause::{Cause, Tag};
use crate::event::{Event, LossCause, Recipient, Tagged};
use crate::layout::{Layout, Queue};
use crate::state::{StackEntry, Target};

use super::{Relayout, bits, card, queue, slot};

impl<L: Layout> Relayout for Recipient<L> {
    type In<M: Layout> = Recipient<M>;

    fn relayout<M: Layout>(&self) -> Recipient<M> {
        match self {
            &Recipient::Player(p) => Recipient::Player(p),
            Recipient::Permanent(r) => Recipient::Permanent(r.relayout()),
        }
    }
}

/// An [`Event::Mana`] in another layout.
fn mana<L: Layout, M: Layout>(e: &Event<L>) -> Event<M> {
    let &Event::Mana {
        player,
        source,
        color,
        amount,
    } = e
    else {
        unreachable!("a mana event")
    };
    Event::Mana {
        player,
        source: source.map(|o| o.relayout()),
        color,
        amount,
    }
}

/// A library's event in another layout: a draw, a scry's or a top-or-bottom choice's result,
/// a shuffle. None names an object.
fn library<L: Layout, M: Layout>(e: &Event<L>) -> Event<M> {
    match *e {
        Event::Draw { player, cards } => Event::Draw { player, cards },
        Event::Scry {
            player,
            top,
            bottom,
        } => Event::Scry {
            player,
            top,
            bottom,
        },
        Event::TopOrBottom { player, top } => Event::TopOrBottom { player, top },
        Event::LibraryShuffled { player } => Event::LibraryShuffled { player },
        _ => unreachable!("a library event"),
    }
}

/// An [`Event::Cast`] in another layout.
fn cast<L: Layout, M: Layout>(e: &Event<L>) -> Event<M> {
    let Event::Cast {
        spell,
        controller,
        targets,
        x,
        costs,
        flashback,
    } = e
    else {
        unreachable!("a cast's event");
    };
    Event::Cast {
        spell: spell.relayout(),
        controller: *controller,
        targets: queue(targets.as_slice(), Target::relayout),
        x: *x,
        costs: *costs,
        flashback: *flashback,
    }
}

/// A block's declaration in another layout.
fn block<L: Layout, M: Layout>(e: &Event<L>) -> Event<M> {
    let &Event::Block {
        player,
        blocker,
        attacker,
    } = e
    else {
        unreachable!("a block");
    };
    Event::Block {
        player,
        blocker: blocker.relayout(),
        attacker: attacker.relayout(),
    }
}

/// An ability's announcement in another layout.
fn ability<L: Layout, M: Layout>(e: &Event<L>) -> Event<M> {
    let Event::Ability {
        source,
        controller,
        card,
        face,
        ability,
        targets,
        x,
    } = e
    else {
        unreachable!("an ability event");
    };
    Event::Ability {
        source: source.map(|r| r.relayout()),
        controller: *controller,
        card: self::card(*card),
        face: *face,
        ability: *ability,
        targets: queue(targets.as_slice(), Target::relayout),
        x: *x,
    }
}

impl<L: Layout> Relayout for LossCause<L> {
    type In<M: Layout> = LossCause<M>;

    fn relayout<M: Layout>(&self) -> LossCause<M> {
        match *self {
            LossCause::Life => LossCause::Life,
            LossCause::Library => LossCause::Library,
            LossCause::Effect {
                source,
                card,
                face,
                ability,
            } => LossCause::Effect {
                source: source.map(|r| r.relayout()),
                card: self::card(card),
                face,
                ability,
            },
            LossCause::Concede => LossCause::Concede,
        }
    }
}

/// A resolution payment's public stack item in another layout.
fn resolution_payment<L: Layout, M: Layout>(event: &Event<L>) -> Event<M> {
    let Event::ResolutionPayment { player, item } = event else {
        unreachable!("a resolution payment");
    };
    Event::ResolutionPayment {
        player: *player,
        item: item.relayout(),
    }
}

/// A stack entry's arrival in another layout.
fn arrival<L: Layout, M: Layout>(entry: &StackEntry<L>) -> Event<M> {
    Event::StackArrival {
        entry: entry.relayout(),
    }
}

impl<L: Layout> Relayout for Tag<L> {
    type In<M: Layout> = Tag<M>;

    fn relayout<M: Layout>(&self) -> Tag<M> {
        Tag {
            cause: match self.cause {
                Cause::Rules => Cause::Rules,
                Cause::Player { player, entry } => Cause::Player { player, entry },
                Cause::Effect { entry } => Cause::Effect { entry },
                Cause::Object { source, entry } => Cause::Object {
                    source: source.relayout(),
                    entry,
                },
            },
            together: self.together,
        }
    }
}

impl<L: Layout> Relayout for Tagged<L> {
    type In<M: Layout> = Tagged<M>;

    fn relayout<M: Layout>(&self) -> Tagged<M> {
        Tagged {
            event: self.event.relayout(),
            tag: self.tag.relayout(),
        }
    }
}

impl<L: Layout> Relayout for Event<L> {
    type In<M: Layout> = Event<M>;

    fn relayout<M: Layout>(&self) -> Event<M> {
        match self {
            &Event::ZoneChange {
                object,
                from,
                to,
                reason,
                library_top,
            } => Event::ZoneChange {
                object: object.relayout(),
                from,
                to,
                reason,
                library_top,
            },
            Event::Damage {
                source,
                to,
                amount,
                combat,
            } => Event::Damage {
                source: source.relayout(),
                to: to.relayout(),
                amount: *amount,
                combat: *combat,
            },
            Event::Draw { .. }
            | Event::Scry { .. }
            | Event::TopOrBottom { .. }
            | Event::LibraryShuffled { .. } => library(self),
            &Event::DrawCard { slot: drawn } => Event::DrawCard { slot: slot(drawn) },
            &Event::Reveal { slot: shown } => Event::Reveal { slot: slot(shown) },
            &Event::LifeGain { player, amount } => Event::LifeGain { player, amount },
            &Event::LifeLoss { player, amount } => Event::LifeLoss { player, amount },
            &Event::CountersAdded {
                object,
                kind,
                amount,
            } => Event::CountersAdded {
                object: object.relayout(),
                kind,
                amount,
            },
            &Event::CountersRemoved {
                object,
                kind,
                amount,
            } => Event::CountersRemoved {
                object: object.relayout(),
                kind,
                amount,
            },
            Event::Tap { object } => Event::Tap {
                object: object.relayout(),
            },
            Event::Untap { object } => Event::Untap {
                object: object.relayout(),
            },
            Event::Mana { .. } => mana(self),
            Event::ResolutionPayment { .. } => resolution_payment(self),
            Event::AbilityFinished => Event::AbilityFinished,
            Event::StackArrival { entry } => arrival(entry),
            &Event::StackDeparture { entry } => Event::StackDeparture { entry },
            &Event::TokensCreated { controller, tokens } => Event::TokensCreated {
                controller,
                tokens: bits(tokens),
            },
            Event::Cast { .. } => cast(self),
            Event::Ability { .. } => ability(self),
            &Event::Attack { player, attackers } => Event::Attack {
                player,
                attackers: bits(attackers),
            },
            Event::Block { .. } => block(self),
            &Event::StepBegins { step, active } => Event::StepBegins { step, active },
            &Event::GainControl { object, player } => Event::GainControl {
                object: object.relayout(),
                player,
            },
            &Event::CoinFlip {
                player,
                won,
                source,
            } => Event::CoinFlip {
                player,
                won,
                source: source.relayout(),
            },
            &Event::Lose { player, cause } => Event::Lose {
                player,
                cause: cause.relayout(),
            },
            &Event::Win { player } => Event::Win { player },
        }
    }
}
