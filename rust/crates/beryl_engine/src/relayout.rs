//! A game's values in another [`Layout`] (architecture §4.1): the same objects in the same
//! slots by number, the same queues in the same order. Widening a [`State<Small>`] into
//! [`crate::Large`] is lossless; values a `Small` game produced fit back into `Small`.

use crate::bitboard::Bitboard;
use crate::decision::{Ask, Candidates, Decision, Opt, Refusal, Step};
use crate::event::{Event, Recipient};
use crate::ids::SlotId;
use crate::layout::{Item, Layout, Queue, Table, index};
use crate::state::{
    CastPermission, Derived, FloatingEffect, Object, ObjectRef, Pending, PendingTrigger, Player,
    Resolving, StackEntry, State, Target, TriggerInfo, Zones,
};

/// A value of one layout's types, as another layout's.
pub trait Relayout {
    /// The value's type in layout `M`.
    type In<M: Layout>;

    /// This value in layout `M`: every slot by its number, every queue in its order.
    ///
    /// # Panics
    ///
    /// If `M` cannot hold it: a slot or index past `M`'s, or a queue past a capacity of `M`.
    /// A value that came from an `M` game always fits.
    fn relayout<M: Layout>(&self) -> Self::In<M>;
}

/// `s` as the slot of the same number.
fn slot<S: SlotId, T: SlotId>(s: S) -> T {
    T::from_index(s.index())
}

/// `b` over the slots of the same numbers.
fn bits<S: SlotId, T: SlotId>(b: Bitboard<S>) -> Bitboard<T> {
    let mut out = Bitboard::<T>::EMPTY;
    let (from, to) = (b.0.as_ref(), out.0.as_mut());
    let n = from.len().min(to.len());
    to[..n].copy_from_slice(&from[..n]);
    assert!(
        from[n..].iter().all(|&w| w == 0),
        "a slot past the layout's"
    );
    out
}

/// The items of `from`, each converted by `f`, in order.
fn queue<T, U: Item + Default, Q: Queue<U>>(from: &[T], f: impl Fn(&T) -> U) -> Q {
    let mut q = Q::default();
    for x in from {
        q.push(f(x)).expect("a queue past the layout's capacity");
    }
    q
}

/// `from`'s entries, each converted by `f`, in a table of `M` otherwise `fill`. Entries past
/// its size must be `fill` unless `live` is the number of entries that matter.
fn table<T, U: Item, Q: Table<U>>(
    from: &[T],
    fill: &U,
    live: Option<usize>,
    f: impl Fn(&T) -> U,
) -> Q {
    let mut q = Q::filled(fill.clone());
    let n = q.as_slice().len();
    for (i, x) in from.iter().enumerate() {
        let x = f(x);
        if i < n {
            q[i] = x;
        } else {
            assert!(
                live.is_some_and(|live| i >= live) || x == *fill,
                "past the layout's slots"
            );
        }
    }
    q
}

impl<L: Layout> Relayout for ObjectRef<L> {
    type In<M: Layout> = ObjectRef<M>;

    fn relayout<M: Layout>(&self) -> ObjectRef<M> {
        ObjectRef {
            slot: slot(self.slot),
            incarnation: self.incarnation,
        }
    }
}

impl<L: Layout> Relayout for Target<L> {
    type In<M: Layout> = Target<M>;

    fn relayout<M: Layout>(&self) -> Target<M> {
        match (self.as_object(), self.as_player()) {
            (Some(r), _) => Target::object(self.spec, r.relayout()),
            (None, Some(p)) => Target::player(self.spec, p),
            (None, None) => Target::default(),
        }
    }
}

impl<L: Layout> Relayout for TriggerInfo<L> {
    type In<M: Layout> = TriggerInfo<M>;

    fn relayout<M: Layout>(&self) -> TriggerInfo<M> {
        TriggerInfo {
            object: self.object.map(|r| r.relayout()),
            player: self.player,
            amount: self.amount,
            lki: self.lki,
        }
    }
}

impl<L: Layout> Relayout for StackEntry<L> {
    type In<M: Layout> = StackEntry<M>;

    fn relayout<M: Layout>(&self) -> StackEntry<M> {
        StackEntry {
            object: self.object.map(|r| r.relayout()),
            controller: self.controller,
            ability: self.ability,
            card: self.card,
            face: self.face,
            info: self.info.relayout(),
            targets: queue(self.targets.as_slice(), Target::relayout),
            x: self.x,
            costs: self.costs,
            exile: self.exile,
            flashback: self.flashback,
        }
    }
}

impl<L: Layout> Relayout for PendingTrigger<L> {
    type In<M: Layout> = PendingTrigger<M>;

    fn relayout<M: Layout>(&self) -> PendingTrigger<M> {
        PendingTrigger {
            source: self.source.map(|r| r.relayout()),
            card: self.card,
            face: self.face,
            ability: self.ability,
            controller: self.controller,
            info: self.info.relayout(),
        }
    }
}

impl<L: Layout> Relayout for CastPermission<L> {
    type In<M: Layout> = CastPermission<M>;

    fn relayout<M: Layout>(&self) -> CastPermission<M> {
        CastPermission {
            object: self.object.relayout(),
            player: self.player,
            extra: self.extra,
            this_turn: self.this_turn,
            exile: self.exile,
            flashback: self.flashback,
        }
    }
}

impl<L: Layout> Relayout for FloatingEffect<L> {
    type In<M: Layout> = FloatingEffect<M>;

    fn relayout<M: Layout>(&self) -> FloatingEffect<M> {
        FloatingEffect {
            source: self.source.relayout(),
            timestamp: self.timestamp,
            duration: self.duration,
            affected: self.affected.relayout(),
            effect: self.effect,
        }
    }
}

impl<L: Layout> Relayout for Object<L> {
    type In<M: Layout> = Object<M>;

    fn relayout<M: Layout>(&self) -> Object<M> {
        Object {
            card: self.card,
            incarnation: self.incarnation,
            zone: self.zone,
            owner: self.owner,
            base_controller: self.base_controller,
            flags: self.flags,
            damage: self.damage,
            counters: self.counters,
            attached_to: self.attached_to.map(slot),
            linked: self.linked.map(slot),
            timestamp: self.timestamp,
            face: self.face,
            combat: self.combat.map(slot),
            battlefield_index: self.battlefield_index,
        }
    }
}

impl<L: Layout> Relayout for Player<L> {
    type In<M: Layout> = Player<M>;

    fn relayout<M: Layout>(&self) -> Player<M> {
        Player {
            life: self.life,
            mana: self.mana,
            lands_played: self.lands_played,
            drew_from_empty: self.drew_from_empty,
            mulligans: self.mulligans,
            mulligan: self.mulligan,
            to_bottom: self.to_bottom,
            emblems: queue(self.emblems.as_slice(), |&c| c),
            sorceries_as_flash: self.sorceries_as_flash,
        }
    }
}

impl<L: Layout> Relayout for Resolving<L> {
    type In<M: Layout> = Resolving<M>;

    fn relayout<M: Layout>(&self) -> Resolving<M> {
        Resolving {
            record: queue(self.record.as_slice(), |&i| index::<M>(i.into())),
            question: self.question.relayout(),
        }
    }
}

impl<L: Layout> Relayout for Pending<L> {
    type In<M: Layout> = Pending<M>;

    fn relayout<M: Layout>(&self) -> Pending<M> {
        match self {
            Pending::None => Pending::None,
            &Pending::PlayOrDraw { chooser } => Pending::PlayOrDraw { chooser },
            Pending::Mulligans => Pending::Mulligans,
            Pending::Discard => Pending::Discard,
            &Pending::LegendRule { kept } => Pending::LegendRule { kept: bits(kept) },
            &Pending::EnterPayingLife { slot: s, life } => Pending::EnterPayingLife {
                slot: slot(s),
                life,
            },
            &Pending::DeclareAttackers { attacker } => Pending::DeclareAttackers {
                attacker: attacker.map(slot),
            },
            &Pending::DeclareBlockers { blocker } => Pending::DeclareBlockers {
                blocker: blocker.map(slot),
            },
            &Pending::CombatDamage { done, lethal } => Pending::CombatDamage {
                done: bits(done),
                lethal: bits(lethal),
            },
            &Pending::Casting(c) => Pending::Casting(c),
            Pending::Resolving(r) => Pending::Resolving(r.relayout()),
            &Pending::CastOffer {
                object,
                player,
                free,
            } => Pending::CastOffer {
                object: object.relayout(),
                player,
                free,
            },
            &Pending::TurningFaceUp { slot: s, remaining } => Pending::TurningFaceUp {
                slot: slot(s),
                remaining,
            },
        }
    }
}

impl<L: Layout> Relayout for Opt<L> {
    type In<M: Layout> = Opt<M>;

    fn relayout<M: Layout>(&self) -> Opt<M> {
        match *self {
            Opt::Pass => Opt::Pass,
            Opt::PlayLand(s) => Opt::PlayLand(slot(s)),
            Opt::Play => Opt::Play,
            Opt::Draw => Opt::Draw,
            Opt::Keep => Opt::Keep,
            Opt::Mulligan => Opt::Mulligan,
            Opt::Trigger(i) => Opt::Trigger(index::<M>(i.into())),
            Opt::Yes => Opt::Yes,
            Opt::No => Opt::No,
            Opt::Done => Opt::Done,
            Opt::Attack(s) => Opt::Attack(slot(s)),
            Opt::Defender(s) => Opt::Defender(s.map(slot)),
            Opt::Block(s) => Opt::Block(slot(s)),
            Opt::Lethal(s) => Opt::Lethal(slot(s)),
            Opt::Cast {
                slot: s,
                face,
                costs,
            } => Opt::Cast {
                slot: slot(s),
                face,
                costs,
            },
            Opt::Activate { slot: s, ability } => Opt::Activate {
                slot: slot(s),
                ability,
            },
            Opt::Mana { slot: s, color } => Opt::Mana {
                slot: slot(s),
                color,
            },
            Opt::Pool(c) => Opt::Pool(c),
            Opt::Number(n) => Opt::Number(n),
            Opt::Plan(n) => Opt::Plan(n),
            Opt::Player(p) => Opt::Player(p),
            Opt::Object(s) => Opt::Object(slot(s)),
            Opt::TurnFaceUp(s) => Opt::TurnFaceUp(slot(s)),
        }
    }
}

impl<L: Layout> Relayout for Ask<L> {
    type In<M: Layout> = Ask<M>;

    fn relayout<M: Layout>(&self) -> Ask<M> {
        match *self {
            Ask::Plain => Ask::Plain,
            Ask::Scry { count } => Ask::Scry { count },
            Ask::Look {
                player,
                zone,
                count,
                to,
                eligible,
            } => Ask::Look {
                player,
                zone,
                count,
                to,
                eligible: bits(eligible),
            },
            Ask::PutBack { count } => Ask::PutBack { count },
            Ask::Fateseal { player } => Ask::Fateseal { player },
            Ask::Take { count, take } => Ask::Take { count, take },
            Ask::Bottom { count } => Ask::Bottom { count },
            Ask::DiscardBasicLand { count } => Ask::DiscardBasicLand { count },
            Ask::TopOrBottom { card } => Ask::TopOrBottom { card: slot(card) },
            Ask::MayMove { card, to } => Ask::MayMove {
                card: slot(card),
                to,
            },
            Ask::Explore => Ask::Explore,
            Ask::Search { max } => Ask::Search { max },
            Ask::MaySearch => Ask::MaySearch,
            Ask::Untap { max } => Ask::Untap { max },
            Ask::Crew { crew, left } => Ask::Crew { crew, left },
            Ask::Waterbend { cost } => Ask::Waterbend { cost },
            Ask::UnlessPays { cost } => Ask::UnlessPays { cost },
            Ask::PayLife { land, life } => Ask::PayLife {
                land: slot(land),
                life,
            },
            Ask::Cleanup { count } => Ask::Cleanup { count },
            Ask::ManifestDread => Ask::ManifestDread,
            Ask::ExileCards { zone, count } => Ask::ExileCards { zone, count },
        }
    }
}

impl<L: Layout> Relayout for Candidates<L> {
    type In<M: Layout> = Candidates<M>;

    fn relayout<M: Layout>(&self) -> Candidates<M> {
        match self {
            &Candidates::Objects(b) => Candidates::Objects(bits(b)),
            Candidates::Options(o) => Candidates::Options(queue(o.as_slice(), Opt::relayout)),
        }
    }
}

impl<L: Layout> Relayout for Decision<L> {
    type In<M: Layout> = Decision<M>;

    fn relayout<M: Layout>(&self) -> Decision<M> {
        Decision {
            player: self.player,
            kind: self.kind,
            candidates: self.candidates.relayout(),
            ask: self.ask.relayout(),
        }
    }
}

impl<L: Layout> Relayout for Step<L> {
    type In<M: Layout> = Step<M>;

    fn relayout<M: Layout>(&self) -> Step<M> {
        match self {
            Step::Decide(d) => Step::Decide(d.relayout()),
            &Step::Over(o) => Step::Over(o),
        }
    }
}

impl<L: Layout> Relayout for Refusal<L> {
    type In<M: Layout> = Refusal<M>;

    fn relayout<M: Layout>(&self) -> Refusal<M> {
        match *self {
            Refusal::Timing { by } => Refusal::Timing { by: by.map(slot) },
            Refusal::LandPlayed => Refusal::LandPlayed,
            Refusal::Mana => Refusal::Mana,
            Refusal::Cost => Refusal::Cost,
            Refusal::Target => Refusal::Target,
        }
    }
}

impl<L: Layout> Relayout for Recipient<L> {
    type In<M: Layout> = Recipient<M>;

    fn relayout<M: Layout>(&self) -> Recipient<M> {
        match self {
            &Recipient::Player(p) => Recipient::Player(p),
            Recipient::Permanent(r) => Recipient::Permanent(r.relayout()),
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
            } => Event::ZoneChange {
                object: object.relayout(),
                from,
                to,
                reason,
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
            &Event::TokensCreated { controller, tokens } => Event::TokensCreated {
                controller,
                tokens: bits(tokens),
            },
            &Event::Cast { spell, controller } => Event::Cast {
                spell: spell.relayout(),
                controller,
            },
            Event::Ability {
                source,
                controller,
                card,
                face,
                ability,
                targets,
                x,
            } => Event::Ability {
                source: source.map(|r| r.relayout()),
                controller: *controller,
                card: *card,
                face: *face,
                ability: *ability,
                targets: queue(targets.as_slice(), Target::relayout),
                x: *x,
            },
            &Event::Attack { player, attackers } => Event::Attack {
                player,
                attackers: bits(attackers),
            },
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
            &Event::Lose { player } => Event::Lose { player },
            &Event::Win { player } => Event::Win { player },
        }
    }
}

impl<L: Layout> Relayout for Zones<L> {
    type In<M: Layout> = Zones<M>;

    fn relayout<M: Layout>(&self) -> Zones<M> {
        Zones {
            members: self.members.map(|owned| owned.map(bits)),
            stack: bits(self.stack),
            library: self
                .library
                .each_ref()
                .map(|q| queue(q.as_slice(), |&s| slot(s))),
            graveyard: self
                .graveyard
                .each_ref()
                .map(|q| queue(q.as_slice(), |&s| slot(s))),
            next_timestamp: self.next_timestamp,
        }
    }
}

impl<L: Layout> Relayout for State<L> {
    type In<M: Layout> = State<M>;

    /// The game in layout `M`, losslessly: every slot keeps its number and object, every
    /// queue its order, every player their knowledge, and the random streams their place, so
    /// the game goes on exactly as it would have, past where `L` runs out.
    fn relayout<M: Layout>(&self) -> State<M> {
        let State {
            objects,
            battlefield,
            derived,
            listening,
            zones,
            players,
            turn,
            stack,
            triggers,
            delayed,
            effects,
            permissions,
            entered_x,
            pending,
            knowledge,
            rng,
            profile,
            budgets,
            flags,
            outcome,
        } = self;
        State {
            objects: table(objects.as_slice(), &Object::EMPTY, None, Object::relayout),
            battlefield: queue(battlefield.as_slice(), |&s| slot(s)),
            // Entries past the battlefield's are stale; copied too, so a widened game equals one
            // that was wide all along.
            derived: table(
                derived.as_slice(),
                &Derived::default(),
                Some(battlefield.len()),
                |&d| d,
            ),
            listening: *listening,
            zones: zones.relayout(),
            players: players.each_ref().map(Player::relayout),
            turn: *turn,
            stack: queue(stack.as_slice(), StackEntry::relayout),
            triggers: queue(triggers.as_slice(), PendingTrigger::relayout),
            delayed: queue(delayed.as_slice(), PendingTrigger::relayout),
            effects: queue(effects.as_slice(), FloatingEffect::relayout),
            permissions: queue(permissions.as_slice(), CastPermission::relayout),
            entered_x: queue(entered_x.as_slice(), |&(r, x)| (r.relayout(), x)),
            pending: pending.relayout(),
            knowledge: knowledge.map(bits),
            rng: *rng,
            profile: *profile,
            budgets: *budgets,
            flags: *flags,
            outcome: *outcome,
        }
    }
}
