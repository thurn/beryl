//! A script as text, one step a line, for the hand-written expectations of tests (plan K24),
//! the specs dump and anyone debugging. It is for reading: it says what each step shows, not
//! every field a replay needs.
//!
//! ```text
//! reset a4
//! fly #7 (Brainstorm) hand>stack cast <- you
//! dwell #7
//! fly #31 library>hand draw <- #7
//! & fly #32 library>hand draw <- #7
//! vanish #7 (Brainstorm) stack>graveyard resolve
//! ```
//!
//! A line is a verb, its subject, what the verb needs said, and ` <- ` its cause. `&` joins a
//! step to the beat of the line above. A card is `#` and its slot, with its name where the
//! seat knows it; an ability is `a` and its number; a player is `you` or `opponent`. `+` and
//! `-` begin and end an arrow, a mark or a lift. Names are the wire's.

use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use super::{Act, Fact, Notice, Script, Span, Step, Stream, Subject, Tally};
use crate::protocol::{
    CardHandling, Combat, Counter, Facing, ManaPool, ObjectView, StackItem, TargetRef, ZoneName,
};

/// A fieldless enum's name on the wire.
fn wire<T: Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => unreachable!("only fieldless enums are named this way"),
    }
}

impl Display for Script {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(reset) = &self.reset {
            f.write_str("reset")?;
            for id in &reset.abilities {
                write!(f, " a{id}")?;
            }
            f.write_str("\n")?;
        }
        for beat in &self.beats {
            for (i, step) in beat.iter().enumerate() {
                writeln!(f, "{}{step}", if i == 0 { "" } else { "& " })?;
            }
        }
        Ok(())
    }
}

impl Display for Step {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.act)?;
        match self.cause {
            Some(cause) => write!(f, " <- {cause}"),
            None => Ok(()),
        }
    }
}

impl Display for Subject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Subject::Object { slot } => write!(f, "#{slot}"),
            Subject::Ability { id } => write!(f, "a{id}"),
            Subject::Player { player } => f.write_str(&wire(player)),
        }
    }
}

/// An object as a line names it: its slot, and its name and face where they are known.
struct Named<'a>(&'a ObjectView);

impl Display for Named<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let ObjectView {
            slot, name, facing, ..
        } = self.0;
        write!(f, "#{slot}")?;
        match (name, facing) {
            (Some(name), Facing::Up) => write!(f, " ({name})"),
            (Some(name), Facing::Down) => write!(f, " ({name}, face down)"),
            (None, Facing::Down) => f.write_str(" (face down)"),
            (None, Facing::Up) => Ok(()),
        }
    }
}

/// Where a moved object now is: its zone, its place in a pile's order, the permanent it is on,
/// and whose side of the battlefield it is on if it only changed sides.
struct Arrival<'a> {
    from: ZoneName,
    object: &'a ObjectView,
    at: Option<u16>,
}

impl<'a> Arrival<'a> {
    fn new(from: ZoneName, object: &'a ObjectView, at: Option<u16>) -> Self {
        Arrival { from, object, at }
    }
}

impl Display for Arrival<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Arrival { from, object, at } = *self;
        write!(f, "{}>{}", wire(from), wire(object.zone))?;
        match (object.zone, at) {
            (ZoneName::Library, Some(0)) => f.write_str(" bottom")?,
            (ZoneName::Library, None) if from != ZoneName::Library => f.write_str(" top")?,
            (_, Some(at)) => write!(f, " @{at}")?,
            (_, None) => {}
        }
        if let Some(host) = object.attached_to {
            write!(f, " on #{host}")?;
        } else if from == object.zone && from == ZoneName::Battlefield {
            write!(f, " {}", wire(object.controller))?;
        }
        Ok(())
    }
}

/// What a spell or ability says of itself beyond its name: its targets, its X and the costs it
/// was cast with.
struct Announced<'a>(&'a StackItem);

impl Display for Announced<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (targets, x) = match self.0 {
            StackItem::Spell { targets, x, .. } | StackItem::Ability { targets, x, .. } => {
                (targets, *x)
            }
        };
        if x > 0 {
            write!(f, " x={x}")?;
        }
        if let StackItem::Spell { paid, .. } = self.0 {
            for cost in paid {
                write!(f, " {}", wire(cost))?;
            }
        }
        for (i, target) in targets.iter().enumerate() {
            f.write_str(if i == 0 { " at " } else { ", " })?;
            match *target {
                TargetRef::Player { player } => f.write_str(&wire(player))?,
                TargetRef::Object { slot } => write!(f, "#{slot}")?,
            }
        }
        Ok(())
    }
}

/// An ability as it goes on the stack: whose face it is on, how it came to be, its controller
/// and its source.
struct Lifted<'a>(&'a StackItem);

impl Display for Lifted<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let StackItem::Ability {
            source,
            name,
            triggered,
            controller,
            ..
        } = self.0
        {
            let kind = if *triggered { "triggered" } else { "activated" };
            write!(f, " ({name}, {kind}) {}", wire(controller))?;
            if let Some(source) = source {
                write!(f, " from #{source}")?;
            }
        }
        write!(f, "{}", Announced(self.0))
    }
}

/// An object's state on the battlefield, for a `set`, whose line has no verb to say what
/// changed.
struct Status<'a>(&'a ObjectView);

impl Display for Status<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let o = self.0;
        f.write_str(&wire(o.zone))?;
        if o.tapped {
            f.write_str(" tapped")?;
        }
        if o.summoning_sick {
            f.write_str(" sick")?;
        }
        if let Some(pt) = o.pt {
            write!(f, " {}/{}", pt.power, pt.toughness)?;
        }
        if o.damage > 0 {
            write!(f, " damage {}", o.damage)?;
        }
        match &o.combat {
            Some(Combat::Attacking { planeswalker, .. }) => {
                f.write_str(" attacking")?;
                if let Some(walker) = planeswalker {
                    write!(f, " #{walker}")?;
                }
            }
            Some(Combat::Blocking { attacker }) => {
                f.write_str(" blocking")?;
                if let Some(attacker) = attacker {
                    write!(f, " #{attacker}")?;
                }
            }
            None => {}
        }
        Ok(())
    }
}

/// A pool as its mana symbols, or `0`.
struct Pool(ManaPool);

impl Display for Pool {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let ManaPool {
            white,
            blue,
            black,
            red,
            green,
            colorless,
        } = self.0;
        let symbols = [
            ("W", white),
            ("U", blue),
            ("B", black),
            ("R", red),
            ("G", green),
            ("C", colorless),
        ];
        if symbols.iter().all(|&(_, amount)| amount == 0) {
            return f.write_str("0");
        }
        symbols
            .iter()
            .try_for_each(|&(symbol, amount)| f.write_str(&symbol.repeat(usize::from(amount))))
    }
}

fn counters(object: &ObjectView, kind: Counter) -> u8 {
    object
        .counters
        .iter()
        .find(|c| c.kind == kind)
        .map_or(0, |c| c.count)
}

fn sign(span: Span) -> char {
    match span {
        Span::Begin => '+',
        Span::End => '-',
    }
}

impl Display for Act {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let verb = self.verb().name();
        match self {
            Act::Fly {
                from,
                why,
                object,
                at,
                item,
            }
            | Act::Appear {
                from,
                why,
                object,
                at,
                item,
            } => {
                let arrival = Arrival::new(*from, object, *at);
                write!(f, "{verb} {} {arrival} {}", Named(object), wire(why))?;
                item.iter()
                    .try_for_each(|item| write!(f, "{}", Announced(item)))
            }
            Act::Vanish {
                from,
                why,
                object: Some(object),
                at,
                ..
            } => {
                let arrival = Arrival::new(*from, object, *at);
                write!(f, "{verb} {} {arrival} {}", Named(object), wire(why))
            }
            Act::Vanish {
                subject, from, why, ..
            } => write!(f, "{verb} {subject} {}>gone {}", wire(from), wire(why)),
            Act::Emerge { id, item } => write!(f, "{verb} a{id}{}", Lifted(item)),
            Act::Flip { object } | Act::Showcase { object } => {
                write!(f, "{verb} {}", Named(object))
            }
            Act::Tap { object } => {
                let verb = if object.tapped { "tap" } else { "untap" };
                write!(f, "{verb} {}", Named(object))
            }
            Act::Strike {
                source, to, amount, ..
            } => write!(f, "{verb} #{source} > {to} {amount}"),
            Act::Streak {
                from,
                to,
                stream,
                amount,
                ..
            } => {
                match from {
                    Some(from) => write!(f, "{verb} {from} > {to}")?,
                    None => write!(f, "{verb} nowhere > {to}")?,
                }
                match stream {
                    Stream::Mana { color } => write!(f, " {} {amount}", wire(color)),
                    Stream::Damage => write!(f, " damage {amount}"),
                }
            }
            Act::Count { tally } => write!(f, "{verb} {tally}"),
            Act::Set { fact } => write!(f, "{verb} {fact}"),
            Act::Dwell { subject } => write!(f, "{verb} {subject}"),
            Act::Announce { notice } => write!(f, "{verb} {notice}"),
            Act::Point {
                span,
                arrow,
                from,
                to,
            } => write!(f, "{}{verb} {} {from} > {to}", sign(*span), arrow.name()),
            Act::Mark { span, mark, on } => {
                write!(f, "{}{verb} {} {on}", sign(*span), mark.name())
            }
            Act::Lift { span, handling } => {
                write!(f, "{}{verb} {}", sign(*span), Handled(*handling))
            }
        }
    }
}

impl Display for Tally {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Tally::Life { player, life } => write!(f, "{} life {life}", wire(player)),
            Tally::Pool { player, mana } => write!(f, "{} pool {}", wire(player), Pool(*mana)),
            Tally::Counters { counter, object } => {
                let count = counters(object, *counter);
                write!(f, "{} {} {count}", Named(object), wire(counter))
            }
            Tally::Stats { object } => {
                write!(f, "{} pt", Named(object))?;
                match object.pt {
                    Some(pt) => write!(f, " {}/{}", pt.power, pt.toughness),
                    None => Ok(()),
                }
            }
        }
    }
}

impl Display for Fact {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Fact::Turn {
                number,
                active,
                step,
            } => write!(f, "turn {number} {} {}", wire(active), wire(step)),
            Fact::Object { object } => write!(f, "{} {}", Named(object), Status(object)),
            Fact::Library { player, order } => {
                write!(f, "{} library {}", wire(player), order.len())
            }
            Fact::Item { subject, item } => write!(f, "{subject}{}", Announced(item)),
        }
    }
}

impl Display for Notice {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            Notice::Turn { active } => write!(f, "turn {}", wire(active)),
            Notice::Scry {
                player,
                top,
                bottom,
            } => write!(f, "scry {} {top} top {bottom} bottom", wire(player)),
            Notice::Coin { player, won } => {
                let result = if won { "won" } else { "lost" };
                write!(f, "coin {} {result}", wire(player))
            }
        }
    }
}

/// The cards a lift takes: whose, from which zone and how many.
struct Handled(CardHandling);

impl Display for Handled {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            CardHandling::Library { side, count, scry } => {
                write!(f, "{} library", wire(side))?;
                match count {
                    Some(count) => write!(f, " {count}")?,
                    None => f.write_str(" all")?,
                }
                if scry == Some(true) {
                    f.write_str(" scry")?;
                }
                Ok(())
            }
            CardHandling::Hand { side } => write!(f, "{} hand", wire(side)),
        }
    }
}
