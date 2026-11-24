//! Events, their causes and their tags in another layout: a part of [`super::Relayout`].
//!
//! Implements: none. Converts events and their causes between storage layouts.

use crate::cause::{Cause, Tag};
use crate::event::{Event, LossCause, Recipient, Tagged};
use crate::layout::{Layout, Queue};
use crate::state::Target;

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

    #[allow(clippy::too_many_lines)] // A line or five per event, in one exhaustive match.
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
            &Event::Draw { player, cards } => Event::Draw { player, cards },
            &Event::Scry {
                player,
                top,
                bottom,
            } => Event::Scry {
                player,
                top,
                bottom,
            },
            &Event::TopOrBottom { player, top } => Event::TopOrBottom { player, top },
            &Event::LibraryShuffled { player } => Event::LibraryShuffled { player },
            &Event::DrawCard { slot: drawn } => Event::DrawCard { slot: slot(drawn) },
            &Event::Reveal {
                player,
                slot: shown,
            } => Event::Reveal {
                player,
                slot: slot(shown),
            },
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
            &Event::Mana {
                player,
                source,
                color,
                amount,
                restricted,
            } => Event::Mana {
                player,
                source: source.map(|o| o.relayout()),
                color,
                amount,
                restricted,
            },
            Event::ResolutionPayment { player, item } => Event::ResolutionPayment {
                player: *player,
                item: item.relayout(),
            },
            Event::AbilityFinished => Event::AbilityFinished,
            Event::GameRestarted => Event::GameRestarted,
            Event::StackArrival { entry } => Event::StackArrival {
                entry: entry.relayout(),
            },
            &Event::StackDeparture { entry } => Event::StackDeparture { entry },
            &Event::TokensCreated { controller, tokens } => Event::TokensCreated {
                controller,
                tokens: bits(tokens),
            },
            Event::Cast {
                spell,
                controller,
                targets,
                x,
                costs,
                flashback,
                from,
            } => Event::Cast {
                spell: spell.relayout(),
                controller: *controller,
                targets: queue(targets.as_slice(), Target::relayout),
                x: *x,
                costs: *costs,
                flashback: *flashback,
                from: *from,
            },
            Event::Ability {
                source,
                controller,
                card: c,
                face,
                ability,
                targets,
                x,
            } => Event::Ability {
                source: source.map(|r| r.relayout()),
                controller: *controller,
                card: card(*c),
                face: *face,
                ability: *ability,
                targets: queue(targets.as_slice(), Target::relayout),
                x: *x,
            },
            &Event::Attack { player, attackers } => Event::Attack {
                player,
                attackers: bits(attackers),
            },
            &Event::Block {
                player,
                blocker,
                attacker,
            } => Event::Block {
                player,
                blocker: blocker.relayout(),
                attacker: attacker.relayout(),
            },
            &Event::TurnPlanChanged(change) => Event::TurnPlanChanged(change),
            &Event::StepBegins { turn, step, active } => Event::StepBegins { turn, step, active },
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
            &Event::Targeted {
                object,
                controller,
                spell,
            } => Event::Targeted {
                object: object.relayout(),
                controller,
                spell: spell.map(|object| object.relayout()),
            },
            &Event::ClassLevel { object, level } => Event::ClassLevel {
                object: object.relayout(),
                level,
            },
            &Event::TurnedFaceUp { object } => Event::TurnedFaceUp {
                object: object.relayout(),
            },
            &Event::Mutated { object, controller } => Event::Mutated {
                object: object.relayout(),
                controller,
            },
        }
    }
}
