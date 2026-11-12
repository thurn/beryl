//! Builds a seat's script from the table it last showed, the view's events and the view's
//! table (plan K23). The events are walked in order, each becoming the steps that show it,
//! played on a working table as they are written; what the view's table then has that no
//! event told (`settle.rs`) closes the script. What an event came from and which events
//! happen together are taken as given from `infer.rs`, the one place that guesses.

use std::collections::BTreeMap;

use super::infer::{self, Origin, Show, TOLD, Told};
use super::moves::{counted, set_counters};
use super::table::Table;
use super::{Act, Fact, Notice, Open, Reset, Script, Step, Stream, Subject, Tally, check, replay};
use super::{mana, spans};
use crate::protocol::{
    CardType, Combat, Counter, GameEvent, ObjectView, PlayerView, Side, StackItem,
    Step as TurnStep, TargetRef, Why, ZoneName,
};

/// What one seat's screen last showed: the session keeps one per seat it sends views to, and
/// asks it for the script beside each view.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// None before the seat's first view, and once forgotten.
    table: Option<Table>,
    /// The number of the latest ability this seat's scripts put on the stack.
    abilities: u32,
    /// The cards the latest view had the seat looking at.
    looked: Vec<u16>,
    /// The arrows, lines and lifts its scripts have begun and not ended: the next script
    /// ends them.
    spans: Vec<Open>,
    /// The arrows ended for good while their spell or ability is still on the stack, by
    /// their ends ([`Builder::part`]).
    broken: Vec<(Subject, Subject)>,
}

impl Screen {
    /// The next script starts over: the view it comes with does not follow from the one
    /// before (an answer taken back).
    pub fn forget(&mut self) {
        self.table = None;
    }

    /// The script that takes the seat from the view it last showed to `view`, which is then
    /// what it shows. `resolving`: the view is in the middle of a spell's or ability's
    /// resolution, which is on top of its table's stack.
    ///
    /// # Panics
    ///
    /// In a debug build, when the script does not replay to `view`'s table: the replay check
    /// (plan K24).
    pub fn script(&mut self, view: &PlayerView, resolving: bool) -> Script {
        let mut after = Table::of(view);
        let script = match self.table.take() {
            None => self.reset(&mut after, view),
            Some(before) => {
                #[cfg(debug_assertions)]
                let shown = (before.clone(), self.spans.clone());
                let told = infer::tell(&before, &after, view, resolving, &self.looked);
                let mut builder = Builder::new(before, &after, view, self);
                builder.lower(view.handling);
                builder.walk(&told, infer::making(view));
                builder.settle();
                builder.close(view);
                let beats = builder.beats();
                let Builder {
                    table,
                    abilities,
                    spans,
                    broken,
                    ..
                } = builder;
                self.abilities = abilities;
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
                    (self.spans, self.broken) = (spans, broken);
                    after = table;
                    script
                } else {
                    self.reset(&mut after, view)
                }
            }
        };
        self.looked = infer::looked(view);
        self.table = Some(after);
        script
    }

    /// Numbers the abilities on `table`'s stack for a script that starts over from it, and
    /// begins at once the spans `view` calls for.
    fn reset(&mut self, table: &mut Table, view: &PlayerView) -> Script {
        let mut abilities = Vec::new();
        for entry in &mut table.stack {
            if matches!(entry.item, StackItem::Ability { .. }) {
                self.abilities += 1;
                entry.id = Some(self.abilities);
                abilities.push(self.abilities);
            }
        }
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
    pub(super) beats: Vec<Vec<Step>>,
    /// For each beat, the spans that end in a beat of their own before it.
    pub(super) ends: Vec<Vec<Step>>,
    /// Whether the next step joins the latest beat.
    pub(super) open: bool,
    abilities: u32,
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
    /// The attackers with no line, what they attack being not yet chosen, and the lines of
    /// combat as the view ends.
    pub(super) aimless: &'a [u16],
    pub(super) lines: Vec<Open>,
    /// Mana waiting for the ability it pays for to be on the stack ([`mana::Owed`]).
    pub(super) owed: Vec<mana::Owed>,
    /// Where the latest step that carries each object's record is: its beat and its place there.
    pub(super) touched: BTreeMap<u16, (usize, usize)>,
    /// Abilities put on the stack ahead of the event that announces them.
    early: Vec<u32>,
    /// Abilities this script's events announced.
    announced: Vec<u32>,
    /// Counts held back until the ability they paid for is on the stack: the slot, the
    /// counter, the change and its cause.
    late: Vec<(u16, Counter, i16, Option<Subject>)>,
}

impl<'a> Builder<'a> {
    fn new(table: Table, after: &'a Table, view: &'a PlayerView, screen: &Screen) -> Self {
        let aimless = view.pending_attackers.as_deref().unwrap_or(&[]);
        Builder {
            table,
            after,
            events: &view.events,
            beats: Vec::new(),
            ends: Vec::new(),
            open: false,
            abilities: screen.abilities,
            spans: screen.spans.clone(),
            broken: screen.broken.clone(),
            moved: Vec::new(),
            landed: Vec::new(),
            begun: Vec::new(),
            ended: Vec::new(),
            aimless,
            lines: spans::combat(after, aimless),
            owed: Vec::new(),
            touched: BTreeMap::new(),
            early: Vec::new(),
            announced: Vec::new(),
            late: Vec::new(),
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
        for i in infer::order(self.events) {
            if !told[i].together {
                self.open = false;
                self.lift_ahead(i, told);
            }
            self.event(i, told[i]);
        }
        self.pay_late();
        if making && let Some(top) = self.table.stack.last().map(super::table::Entry::subject) {
            self.drain(Side::You, top, Some(Subject::Player { player: Side::You }));
        }
    }

    /// Names what `origin` is as the next step is written.
    fn cause(&self, origin: Origin) -> Option<Subject> {
        match origin {
            Origin::Rules => None,
            Origin::Player(player) => Some(Subject::Player { player }),
            Origin::Object(slot) => Some(Subject::Object { slot }),
            Origin::Ability => self.resolving_ability().map(|id| Subject::Ability { id }),
            Origin::Top => self.table.stack.last().map(super::table::Entry::subject),
        }
    }

    /// The ability on top of the stack, if one is and it is gone by the view's end: one still
    /// there has not resolved, and the ability that did was never on this seat's table.
    fn resolving_ability(&self) -> Option<u32> {
        let id = self.table.stack.last()?.id?;
        let stays = infer::pair(&self.table.stack, &self.after.stack).pop()?;
        stays.is_none().then_some(id)
    }

    /// Which entries of the view's stack the table has already.
    fn present(&self) -> Vec<bool> {
        let mut present = vec![false; self.after.stack.len()];
        for at in infer::pair(&self.table.stack, &self.after.stack)
            .into_iter()
            .flatten()
        {
            present[at] = true;
        }
        present
    }

    /// Puts the ability `item` on the stack, as a new one.
    pub(super) fn lift(&mut self, item: StackItem, cause: Option<Subject>) -> u32 {
        self.abilities += 1;
        let id = self.abilities;
        self.emit(Act::Emerge { id, item }, cause);
        id
    }

    /// Puts on the stack, in a beat of their own, the abilities the view's stack has up to
    /// its entry `through` that the table lacks: the events never say that a triggered
    /// ability with no target went on the stack, and what goes on above one must find it
    /// there.
    pub(super) fn lift_through(&mut self, through: usize) {
        let present = self.present();
        let after = self.after;
        let mut lifted = false;
        for (entry, present) in after.stack.iter().zip(present).take(through + 1) {
            if present || !matches!(entry.item, StackItem::Ability { .. }) {
                continue;
            }
            if !lifted {
                self.open = false;
                lifted = true;
            }
            let id = self.lift(entry.item.clone(), activator(&entry.item));
            self.early.push(id);
        }
        if lifted {
            self.open = false;
        }
    }

    /// Before the permanents that leave in the beat event `i` begins: the triggered
    /// abilities that come out of them lift first.
    fn lift_ahead(&mut self, i: usize, told: &[Told]) {
        let after = self.after;
        let leaving = (i..told.len())
            .take_while(|&j| j == i || told[j].together)
            .filter_map(|j| match self.events[j] {
                GameEvent::ZoneChange {
                    slot,
                    from: ZoneName::Battlefield,
                    to,
                    ..
                } if to != ZoneName::Battlefield => Some(slot),
                _ => None,
            });
        let through = leaving
            .filter_map(|slot| {
                let present = self.present();
                (0..after.stack.len())
                    .rev()
                    .find(|&at| !present[at] && infer::lifts_from(&after.stack[at].item, slot))
            })
            .max();
        if let Some(through) = through {
            self.lift_through(through);
        }
    }

    fn event(&mut self, i: usize, told: Told) {
        let cause = self.cause(told.cause);
        let still = told.show == Show::Still;
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
            } => self.counters(told, slot, counter, i16::from(amount), cause),
            &GameEvent::CountersRemoved {
                slot,
                counter,
                amount,
            } => self.counters(told, slot, counter, -i16::from(amount), cause),
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
            } => self.cast(told, *slot, *controller, item, cause),
            GameEvent::Ability { item, .. } => {
                let subject = self.announce(item, cause);
                self.pay_owed(i, subject);
                if let (true, Some(Subject::Player { player })) = (told.drains, activator(item)) {
                    self.drain(player, subject, cause);
                }
                self.pay_late();
                self.dwell(told, subject);
            }
            GameEvent::AbilityFinished => self.finish(cause),
            GameEvent::Attack { attackers, .. } => self.attack(attackers, cause),
            &GameEvent::StepBegins { step, active } => self.step_begins(i, step, active),
            &GameEvent::GainControl { slot, player } => self.gain_control(slot, player, cause),
            &GameEvent::CoinFlip { player, won, .. } => {
                self.announce_notice(Notice::Coin { player, won }, cause);
            }
            &GameEvent::Scry {
                player,
                top,
                bottom,
            } => {
                let notice = Notice::Scry {
                    player,
                    top,
                    bottom,
                };
                self.announce_notice(notice, cause);
            }
            &GameEvent::LibraryShuffled { player } => self.shuffle(player, cause),
            &GameEvent::Reveal { slot } => self.reveal(i, slot, cause),
            &GameEvent::Mana {
                player,
                source,
                color,
                amount,
            } => self.mana(told, (player, source), color, amount, cause),
            GameEvent::ResolutionPayment { player, item } => {
                if let (true, Some(to)) = (told.drains, self.paid_into(item)) {
                    self.drain(*player, to, cause);
                }
            }
            // How the game ended is chrome.
            GameEvent::TokensCreated { .. } | GameEvent::Lose { .. } | GameEvent::Win { .. } => {}
        }
    }

    fn announce_notice(&mut self, notice: Notice, cause: Option<Subject>) {
        self.emit(Act::Announce { notice }, cause);
    }

    /// The spell in `slot` is announced: as its cast completed it, if the table has it another
    /// way.
    fn cast(
        &mut self,
        told: Told,
        slot: u16,
        controller: Side,
        item: &StackItem,
        cause: Option<Subject>,
    ) {
        let subject = Subject::Object { slot };
        let entry = self.table.stack.iter().find(|e| e.subject() == subject);
        let Some(entry) = entry else { return };
        if entry.item != *item {
            let item = item.clone();
            let fact = Fact::Item { subject, item };
            self.emit(Act::Set { fact }, cause);
        }
        if told.drains {
            self.drain(controller, subject, cause);
        }
        self.dwell(told, subject);
    }

    /// The ability on top of the stack finished resolving and leaves it.
    fn finish(&mut self, cause: Option<Subject>) {
        if let Some(id) = self.resolving_ability() {
            self.emit(
                Act::Vanish {
                    subject: Subject::Ability { id },
                    from: ZoneName::Stack,
                    why: Why::Resolve,
                    object: None,
                    at: None,
                },
                cause,
            );
        }
    }

    /// The attackers are declared: each that did not tap to attack takes its part in combat
    /// with no motion.
    fn attack(&mut self, attackers: &[u16], cause: Option<Subject>) {
        for &slot in attackers {
            if let Some(mut object) = self.permanent(slot)
                && object.combat.is_none()
            {
                object.combat = Some(self.attacking(slot));
                let fact = Fact::Object { object };
                self.emit(Act::Set { fact }, cause);
            }
        }
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

    /// The opponent's spell or ability is left to be read, in a beat of its own.
    fn dwell(&mut self, told: Told, subject: Subject) {
        if told.show == Show::Dwell {
            self.open = false;
            self.emit(Act::Dwell { subject }, None);
            self.open = false;
        }
    }

    fn permanent(&self, slot: u16) -> Option<ObjectView> {
        self.table
            .objects
            .get(&slot)
            .filter(|o| o.zone == ZoneName::Battlefield)
            .cloned()
    }

    /// The part in combat an attacker in `slot` takes as it is declared: the view's, not yet
    /// blocked.
    fn attacking(&self, slot: u16) -> Combat {
        match self
            .after
            .objects
            .get(&slot)
            .and_then(|o| o.combat.as_ref())
        {
            Some(&Combat::Attacking {
                planeswalker,
                target_departed,
                ..
            }) => Combat::Attacking {
                planeswalker,
                target_departed,
                blocked: false,
            },
            _ => Combat::Attacking {
                planeswalker: None,
                target_departed: None,
                blocked: false,
            },
        }
    }

    fn tap(&mut self, slot: u16, tapped: bool, cause: Option<Subject>) {
        let Some(mut object) = self.permanent(slot) else {
            return;
        };
        if object.tapped == tapped {
            return;
        }
        object.tapped = tapped;
        // An attacker taps as it is declared: its record says both.
        let attacks = |e: &GameEvent| matches!(e, GameEvent::Attack { attackers, .. } if attackers.contains(&slot));
        if tapped && object.combat.is_none() && self.events.iter().any(attacks) {
            object.combat = Some(self.attacking(slot));
        }
        self.emit(Act::Tap { object }, cause);
    }

    /// The counts held back for the ability they paid for, now that it is on the stack, in a
    /// beat of their own.
    fn pay_late(&mut self) {
        for (slot, counter, by, cause) in std::mem::take(&mut self.late) {
            self.open = false;
            self.counters(TOLD, slot, counter, by, cause);
        }
    }

    fn counters(
        &mut self,
        told: Told,
        slot: u16,
        counter: Counter,
        by: i16,
        cause: Option<Subject>,
    ) {
        if told.show == Show::Late {
            self.late.push((slot, counter, by, cause));
            return;
        }
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

    /// Where on the table's stack the ability `item`, which an event has yet to announce,
    /// already is: put there ahead of its event, or in the making on top.
    pub(super) fn making(&self, item: &StackItem) -> Option<usize> {
        let ahead = self.table.stack.iter().position(|e| {
            e.id.is_some_and(|id| self.early.contains(&id)) && infer::same(&e.item, item)
        });
        ahead.or_else(|| {
            let top = self.table.stack.last()?;
            let fresh = top.id.is_some_and(|id| self.announced.contains(&id));
            (!fresh && infer::same(&top.item, item)).then(|| self.table.stack.len() - 1)
        })
    }

    /// The ability an event says was activated, or put on the stack with its targets: the
    /// one the table has in the making, as it now is, or a new one.
    fn announce(&mut self, item: &StackItem, cause: Option<Subject>) -> Subject {
        if let Some(at) = self.making(item) {
            let entry = &self.table.stack[at];
            let subject = entry.subject();
            self.early.retain(|&id| Some(id) != entry.id);
            self.announced.extend(entry.id);
            if entry.item != *item {
                let item = item.clone();
                self.emit(
                    Act::Set {
                        fact: Fact::Item { subject, item },
                    },
                    cause,
                );
            }
            return subject;
        }
        let present = self.present();
        let after = self.after;
        let at = (0..after.stack.len())
            .find(|&at| !present[at] && infer::same(&after.stack[at].item, item));
        if let Some(below) = at.and_then(|at| at.checked_sub(1)) {
            self.lift_through(below);
        }
        let id = self.lift(item.clone(), cause);
        self.announced.push(id);
        Subject::Ability { id }
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
