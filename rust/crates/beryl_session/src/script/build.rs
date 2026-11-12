//! Builds a seat's script from the table it last showed, the view's events and the view's
//! table (plan K23). The events are walked in order, each becoming the steps that show it,
//! played on a working table as they are written; what the view's table then has that no
//! event told (`settle.rs`) closes the script. What an event came from, what mana pays for
//! and which spell or ability on the stack an event is about are the engine's word
//! ([`Heard`]), and so is which events happen at once; how they are staged beyond that is
//! `staging.rs`'s.

use std::collections::BTreeMap;

use super::hear::{Cause, Heard, Move};
use super::moves::{counted, set_counters};
use super::spans;
use super::stack::Held;
use super::staging::{self, Told};
use super::table::Table;
use super::{Act, Fact, Notice, Open, Reset, Script, Step, Stream, Subject, Tally, check, replay};
use crate::protocol::{
    CardType, Counter, GameEvent, ObjectView, PlayerView, Side, StackItem, Step as TurnStep,
    TargetRef, Why, ZoneName,
};

/// What one seat's screen last showed: the session keeps one per seat it sends views to, and
/// asks it for the script beside each view.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// None before the seat's first view, and once forgotten.
    table: Option<Table>,
    /// The abilities the engine said arrived on the stack that the seat's views have not
    /// shown yet.
    held: Vec<Held>,
    /// The cards the latest view had the seat looking at.
    looked: Vec<u16>,
    /// The arrows, lines and lifts its scripts have begun and not ended: the next script
    /// ends them.
    spans: Vec<Open>,
    /// The arrows ended for good while their spell or ability is still on the stack, by
    /// their ends ([`Builder::part`]): no guess, but what a later view's stack cannot say,
    /// its targets naming slots alone. One is the engine's word that the target is a new
    /// object, a zone change told of it
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves
    /// from one zone to another becomes a new object"); the other is staging, a spell or
    /// ability done with its target pointing at it no more.
    broken: Vec<(Subject, Subject)>,
    /// The attackers whose target the latest view had still to be chosen
    /// ([`PlayerView::pending_attackers`]).
    aimless: Vec<u16>,
}

impl Screen {
    /// The next script starts over: the view it comes with does not follow from the one
    /// before (an answer taken back).
    pub fn forget(&mut self) {
        self.table = None;
    }

    /// The script that takes the seat from the view it last showed to `view`, which is then
    /// what it shows. `heard`: what the engine said of the view's events and of its stack.
    ///
    /// # Panics
    ///
    /// In a debug build, when the script does not replay to `view`'s table: the replay check
    /// (plan K24).
    pub fn script(&mut self, view: &PlayerView, heard: &Heard) -> Script {
        let mut after = Table::named(view, &heard.stack);
        // An ability nothing named is told apart by its place, which no script's number is:
        // a view whose stack the engine's events were not followed to.
        for (at, entry) in after.stack.iter_mut().enumerate() {
            let unnamed = matches!(entry.item, StackItem::Ability { .. }) && entry.id.is_none();
            if unnamed {
                entry.id = Some(u32::MAX - u32::try_from(at).unwrap_or(0));
            }
        }
        let script = match self.table.take() {
            None => self.reset(&mut after, view),
            Some(before) => {
                #[cfg(debug_assertions)]
                let shown = (before.clone(), self.spans.clone());
                let pregame = before.turn.number == 0;
                let mut builder = Builder::new(before, &after, view, heard, self);
                let told = staging::tell(view, heard, &builder.causes, &self.looked, pregame);
                builder.lower(view.handling);
                builder.walk(&told, staging::making(view));
                builder.settle();
                builder.close(view);
                let beats = builder.beats();
                let Builder {
                    table,
                    spans,
                    broken,
                    held,
                    ..
                } = builder;
                #[allow(unused_mut)] // A test may plant a fault in it.
                let mut script = Script { reset: None, beats };
                #[cfg(test)]
                if let Some(fault) = PLANTED.get() {
                    fault(&mut script);
                }
                #[cfg(debug_assertions)]
                verify(shown, &script, &after, &spans);
                // A release build starts the stage over rather than leave it out of step.
                if table.shows(&after) {
                    (self.spans, self.broken, self.held) = (spans, broken, held);
                    after = table;
                    script
                } else {
                    self.reset(&mut after, view)
                }
            }
        };
        self.looked = staging::looked(view);
        self.aimless = view.pending_attackers.clone().unwrap_or_default();
        self.table = Some(after);
        script
    }

    /// The script that starts over from `table`, saying the numbers of the abilities on its
    /// stack and beginning at once the spans `view` calls for.
    fn reset(&mut self, table: &mut Table, view: &PlayerView) -> Script {
        let abilities = table.stack.iter().filter_map(|e| e.id).collect();
        self.held.clear();
        self.spans = spans::wanted(table, view.pending_attackers.as_deref().unwrap_or(&[]));
        self.spans
            .extend(view.handling.map(|handling| Open::Lift { handling }));
        self.broken.clear();
        let script = Script {
            reset: Some(Reset { abilities }),
            beats: spans::shown(&self.spans),
        };
        debug_assert_eq!(
            check::carried(table.clone(), &[], &script).as_ref(),
            Ok(&self.spans),
            "a reset shows its screen's spans"
        );
        script
    }
}

#[cfg(test)]
thread_local! {
    /// A fault planted in every script this thread's screens build, before the checks see
    /// it: the fuzz's test that a broken builder shrinks to a few choices (`fuzz.rs`).
    pub(crate) static PLANTED: std::cell::Cell<Option<fn(&mut Script)>> =
        const { std::cell::Cell::new(None) };
}

/// The checks every script a debug build makes is held to (plan K24): it replays from the
/// table `shown` to the view's, and it keeps the rule invariants over the spans `shown`
/// had open, leaving open the ones the builder says it does.
#[cfg(debug_assertions)]
fn verify((before, open): (Table, Vec<Open>), script: &Script, after: &Table, spans: &[Open]) {
    if let Some(failure) = replay::check(before.clone(), script, after) {
        panic!("a script does not replay to its view: {failure}");
    }
    match check::carried(before, &open, script) {
        Err(broken) => panic!("a script breaks a rule: {broken}\nin the script\n{script}"),
        Ok(carried) => assert!(
            carried == spans,
            "a script leaves open {carried:?}, and its screen holds {spans:?}\n{script}"
        ),
    }
}

/// A script in the making.
pub(super) struct Builder<'a> {
    /// The table as the steps so far leave it.
    pub(super) table: Table,
    /// The table of the view the script comes with.
    pub(super) after: &'a Table,
    pub(super) events: &'a [GameEvent],
    /// What the engine said each event came from, in step with them.
    pub(super) causes: Vec<Cause>,
    pub(super) moves: &'a [(usize, Move)],
    pub(super) beats: Vec<Vec<Step>>,
    /// For each beat, the spans that end in a beat of their own before it.
    pub(super) ends: Vec<Vec<Step>>,
    /// Whether the next step joins the latest beat.
    pub(super) open: bool,
    pub(super) held: Vec<Held>,
    /// The spans open as the steps so far leave them, and the arrows ended for good
    /// ([`Screen`]'s, carried through this script).
    pub(super) spans: Vec<Open>,
    pub(super) broken: Vec<(Subject, Subject)>,
    /// What the open beat has moved; of that, what it put on the stack; and the spans it
    /// began and ended.
    pub(super) moved: Vec<Subject>,
    pub(super) landed: Vec<Subject>,
    pub(super) begun: Vec<Open>,
    pub(super) ended: Vec<Open>,
    /// The attackers with no line: what they attack is not yet chosen as the view ends, or
    /// was not as the view before ended and no event has declared it since, so their
    /// records may say no more than that they attack and a line drawn from one would point
    /// at the player before the planeswalker (by-m0wf).
    pub(super) aimless: Vec<u16>,
    /// The attackers whose target is still to be chosen as the view ends.
    pub(super) pending: &'a [u16],
    /// The players whose pools' losses have gone into what they paid for, and whether a
    /// step ends in the script, which empties every pool.
    drained: Vec<Side>,
    empties: bool,
    /// Where the latest step that carries each object's record is: its beat and its place there.
    pub(super) touched: BTreeMap<u16, (usize, usize)>,
}

impl<'a> Builder<'a> {
    fn new(
        table: Table,
        after: &'a Table,
        view: &'a PlayerView,
        heard: &'a Heard,
        screen: &Screen,
    ) -> Self {
        let pending = view.pending_attackers.as_deref().unwrap_or(&[]);
        let mut aimless = screen.aimless.clone();
        aimless.extend(pending);
        // An event nothing was said of is the rules'.
        let mut causes = heard.causes.clone();
        causes.resize(view.events.len(), Cause::Rules);
        let begins = |e: &GameEvent| matches!(e, GameEvent::StepBegins { .. });
        Builder {
            table,
            after,
            events: &view.events,
            causes,
            moves: &heard.moves,
            beats: Vec::new(),
            ends: Vec::new(),
            open: false,
            held: screen.held.clone(),
            spans: screen.spans.clone(),
            broken: screen.broken.clone(),
            moved: Vec::new(),
            landed: Vec::new(),
            begun: Vec::new(),
            ended: Vec::new(),
            aimless,
            pending,
            drained: Vec::new(),
            empties: view.events.iter().any(begins),
            touched: BTreeMap::new(),
        }
    }

    /// The beat the next step joins: the open one, or a new one.
    pub(super) fn beat(&mut self) -> usize {
        if !self.open {
            self.beats.push(Vec::new());
            self.ends.push(Vec::new());
            self.open = true;
            self.moved.clear();
            self.landed.clear();
            self.begun.clear();
            self.ended.clear();
        }
        self.beats.len() - 1
    }

    /// Writes a step and plays it on the table, with the spans it ends and begins
    /// (`spans.rs`). A step the table cannot take is left out: what it would have shown is
    /// then settled at the end.
    pub(super) fn emit(&mut self, act: Act, cause: Option<Subject>) {
        let moving = check::moves(&act);
        let lands = check::lands(&self.table, &act);
        let zone = |table: &Table| match moving {
            Some(Subject::Object { slot }) => table.objects.get(&slot).map(|o| o.zone),
            _ => None,
        };
        let was = zone(&self.table);
        if self.table.apply(&act).is_err() {
            return;
        }
        if let Some(subject) = moving {
            self.part(subject, was != zone(&self.table), cause);
        }
        let beat = self.beat();
        if let Some(object) = act.object() {
            self.touched
                .insert(object.slot, (beat, self.beats[beat].len()));
        }
        self.beats[beat].push(Step { act, cause });
        if let Some(subject) = moving {
            self.moved.push(subject);
            if lands {
                self.landed.push(subject);
            }
        }
        self.sync();
    }

    /// `making`: the viewer is in the middle of a cast or activation no event announces yet.
    fn walk(&mut self, told: &[Told], making: bool) {
        let mut moves = self.moves.iter().peekable();
        for i in staging::order(self.events, told) {
            while let Some((_, arrives)) = moves.next_if(|(before, _)| *before <= i) {
                self.stack_move(arrives);
            }
            if !told[i].together {
                self.open = false;
                self.lift_ahead(i, told);
            }
            self.event(i, told[i]);
        }
        for (_, leaves) in moves {
            self.stack_move(leaves);
        }
        if making && let Some(top) = self.table.stack.last().map(super::table::Entry::subject) {
            self.drain(Side::You, top, Some(Subject::Player { player: Side::You }));
        }
    }

    /// Whether what `player`'s pool lost since the view before goes into what they now pay
    /// for: the first of theirs in a script, with no step ending in it to empty the pool
    /// ([CR 500.5](https://yawgatog.com/resources/magic-rules/#R5005): "As a step or phase
    /// ends ... any unspent mana left in a player's mana pool empties"). The engine has no
    /// event for mana spent.
    pub(super) fn drains(&mut self, player: Side) -> bool {
        let drains = !self.empties && !self.drained.contains(&player);
        if drains {
            self.drained.push(player);
        }
        drains
    }

    fn event(&mut self, i: usize, told: Told) {
        let said = self.causes[i];
        self.unhold(said);
        let cause = said.subject();
        let still = told.still;
        let events = self.events;
        match &events[i] {
            &GameEvent::ZoneChange {
                slot,
                from,
                to,
                reason,
                library_top,
            } => self.zone_change(i, slot, (from, to), reason, library_top, still, cause),
            GameEvent::Draw {
                player,
                cards,
                slots,
            } => {
                // From the top of the library, its list's end.
                let slots = slots.clone().unwrap_or_else(|| {
                    let library = &self.table.piles(*player).library;
                    library
                        .iter()
                        .rev()
                        .take(usize::from(*cards))
                        .copied()
                        .collect()
                });
                for slot in slots {
                    let zones = (ZoneName::Library, ZoneName::Hand);
                    self.zone_change(i, slot, zones, Why::Draw, None, still, cause);
                }
            }
            &GameEvent::Tap { slot } => self.tap(slot, true, cause),
            &GameEvent::Untap { slot } => self.tap(slot, false, cause),
            &GameEvent::CountersAdded {
                slot,
                counter,
                amount,
            } => self.counters(slot, counter, i16::from(amount), cause),
            &GameEvent::CountersRemoved {
                slot,
                counter,
                amount,
            } => self.counters(slot, counter, -i16::from(amount), cause),
            &GameEvent::LifeGain { player, amount } => self.life(player, i16::from(amount), cause),
            &GameEvent::LifeLoss { player, amount } => {
                self.life(player, -i16::from(amount), cause);
            }
            GameEvent::Damage {
                source,
                to,
                amount,
                combat,
            } => self.damage(*source, to, *amount, *combat, cause),
            GameEvent::Cast {
                slot,
                item,
                controller,
            } => self.cast(*slot, *controller, item, cause),
            GameEvent::Ability { item, .. } => self.announce(item, said),
            GameEvent::Attack { attackers, .. } => self.attack(attackers, cause),
            &GameEvent::Block {
                blocker, attacker, ..
            } => self.block(blocker, attacker, cause),
            &GameEvent::StepBegins { step, active } => self.step_begins(i, step, active),
            &GameEvent::GainControl { slot, player } => self.gain_control(slot, player, cause),
            GameEvent::CoinFlip { .. } | GameEvent::Scry { .. } => {
                if let Some(notice) = notice(&events[i]) {
                    self.emit(Act::Announce { notice }, cause);
                }
            }
            &GameEvent::LibraryShuffled { player } => self.shuffle(player, cause),
            &GameEvent::Reveal { slot } => self.reveal(i, slot, cause),
            &GameEvent::Mana {
                player,
                source,
                color,
                amount,
            } => self.mana(said, (player, source), color, amount),
            GameEvent::ResolutionPayment { player, .. } => {
                let into = said.entry().filter(|&entry| self.stacked(entry));
                if let (true, Some(to)) = (self.drains(*player), into) {
                    self.drain(*player, to, cause);
                }
            }
            // An ability's departure says it finished ([`Move::Departure`]), and how the game
            // ended is chrome.
            GameEvent::AbilityFinished
            | GameEvent::TokensCreated { .. }
            | GameEvent::Lose { .. }
            | GameEvent::Win { .. } => {}
        }
    }

    /// The spell in `slot` is announced: as its cast completed it, if the table has it another
    /// way.
    fn cast(&mut self, slot: u16, controller: Side, item: &StackItem, cause: Option<Subject>) {
        let subject = Subject::Object { slot };
        let entry = self.table.stack.iter().find(|e| e.subject() == subject);
        let Some(entry) = entry else { return };
        if entry.item != *item {
            let item = item.clone();
            let fact = Fact::Item { subject, item };
            self.emit(Act::Set { fact }, cause);
        }
        if self.drains(controller) {
            self.drain(controller, subject, cause);
        }
        self.dwell(controller, subject);
    }

    fn gain_control(&mut self, slot: u16, player: Side, cause: Option<Subject>) {
        if let Some(mut object) = self.permanent(slot) {
            object.controller = player;
            self.emit(
                Act::Fly {
                    from: ZoneName::Battlefield,
                    why: Why::Effect,
                    object,
                    at: None,
                    item: None,
                },
                cause,
            );
        }
    }

    /// A library is shuffled: its order is the view's end's, with the cards that leave it
    /// meanwhile on top, where a draw takes them from.
    fn shuffle(&mut self, player: Side, cause: Option<Subject>) {
        let end = &self.after.piles(player).library;
        let now = &self.table.piles(player).library;
        let stay = end.iter().filter(|s| now.contains(s));
        let leave = now.iter().filter(|s| !end.contains(s));
        let order = stay.chain(leave).copied().collect();
        let fact = Fact::Library { player, order };
        self.emit(Act::Set { fact }, cause);
    }

    /// The opponent's spell or ability, gone by the time the view shows, is left to be read,
    /// in a beat of its own.
    pub(super) fn dwell(&mut self, controller: Side, subject: Subject) {
        let stays = self.after.stack.iter().any(|e| e.subject() == subject);
        if controller == Side::Opponent && !stays {
            self.open = false;
            self.emit(Act::Dwell { subject }, None);
            self.open = false;
        }
    }

    pub(super) fn permanent(&self, slot: u16) -> Option<ObjectView> {
        self.table
            .objects
            .get(&slot)
            .filter(|o| o.zone == ZoneName::Battlefield)
            .cloned()
    }

    fn counters(&mut self, slot: u16, counter: Counter, by: i16, cause: Option<Subject>) {
        let Some(mut object) = self.table.objects.get(&slot).cloned() else {
            return;
        };
        let had = counted(&object.counters, counter);
        let count = i16::from(had) + by;
        let count = u8::try_from(count.max(0)).unwrap_or(u8::MAX);
        set_counters(&mut object.counters, counter, count);
        // The counter's own change to its creature shows with it
        // ([CR 122.1a](https://yawgatog.com/resources/magic-rules/#R1221a): "A +X/+Y counter
        // on a creature ... adds X to that object's power and Y to that object's toughness.
        // Similarly, -X/-Y counters subtract from power and toughness").
        let each = match counter {
            Counter::PlusOne => i16::from(count) - i16::from(had),
            Counter::MinusOne => i16::from(had) - i16::from(count),
            _ => 0,
        };
        if let (Some(pt), Ok(each)) = (&mut object.pt, i8::try_from(each)) {
            pt.power = pt.power.saturating_add(each);
            pt.toughness = pt.toughness.saturating_add(each);
        }
        let tally = Tally::Counters { counter, object };
        self.emit(Act::Count { tally }, cause);
    }

    fn life(&mut self, player: Side, by: i16, cause: Option<Subject>) {
        let life = self.table.piles(player).life.saturating_add(by);
        let tally = Tally::Life { player, life };
        self.emit(Act::Count { tally }, cause);
    }

    fn damage(
        &mut self,
        source: u16,
        to: &TargetRef,
        amount: u8,
        combat: bool,
        cause: Option<Subject>,
    ) {
        // Damage is marked on a creature and takes loyalty off a planeswalker
        // ([CR 120.3](https://yawgatog.com/resources/magic-rules/#R1203)); neither has an
        // event of its own, nor has the life a player loses to it.
        let (to, object) = match *to {
            TargetRef::Player { player } => (Subject::Player { player }, None),
            TargetRef::Object { slot } => {
                let mut object = self.table.objects.get(&slot).cloned();
                if let Some(object) = &mut object {
                    if object.pt.is_some() {
                        object.damage = object.damage.saturating_add(amount);
                    }
                    if object.types.contains(&CardType::Planeswalker) {
                        let loyalty = counted(&object.counters, Counter::Loyalty);
                        let loyalty = loyalty.saturating_sub(amount);
                        set_counters(&mut object.counters, Counter::Loyalty, loyalty);
                    }
                }
                (Subject::Object { slot }, object)
            }
        };
        let act = if combat {
            Act::Strike {
                source,
                to,
                amount,
                object,
            }
        } else {
            let on_table = self.table.objects.contains_key(&source);
            Act::Streak {
                from: cause.or(on_table.then_some(Subject::Object { slot: source })),
                to,
                stream: Stream::Damage,
                amount,
                object,
            }
        };
        self.emit(act, cause);
        if let Subject::Player { player } = to {
            self.life(player, -i16::from(amount), cause);
        }
    }

    fn step_begins(&mut self, i: usize, step: TurnStep, active: Side) {
        let combat = matches!(
            step,
            TurnStep::BeginningOfCombat
                | TurnStep::DeclareAttackers
                | TurnStep::DeclareBlockers
                | TurnStep::FirstStrikeDamage
                | TurnStep::CombatDamage
                | TurnStep::EndOfCombat
        );
        if !combat {
            self.end_combat();
        }
        let later = self.events[i + 1..]
            .iter()
            .any(|e| matches!(e, GameEvent::StepBegins { .. }));
        let was = self.table.turn.number;
        let number = if later {
            was + u16::from(step == TurnStep::Untap)
        } else {
            self.after.turn.number
        };
        let fact = Fact::Turn {
            number,
            active,
            step,
        };
        self.emit(Act::Set { fact }, None);
        if number != was {
            let notice = Notice::Turn { active };
            self.emit(Act::Announce { notice }, None);
        }
    }
}

/// What a pill or a banner says of `event`, if it is one that is announced.
fn notice(event: &GameEvent) -> Option<Notice> {
    match *event {
        GameEvent::CoinFlip {
            player,
            won,
            source,
        } => Some(Notice::Coin {
            player,
            won,
            source,
        }),
        GameEvent::Scry {
            player,
            top,
            bottom,
        } => Some(Notice::Scry {
            player,
            top,
            bottom,
        }),
        _ => None,
    }
}

/// Who put the ability `item` on the stack by their own action: the controller of an
/// activated one. A triggered one the rules put there.
pub(super) fn activator(item: &StackItem) -> Option<Subject> {
    match *item {
        StackItem::Ability {
            triggered: false,
            controller,
            ..
        } => Some(Subject::Player { player: controller }),
        _ => None,
    }
}
