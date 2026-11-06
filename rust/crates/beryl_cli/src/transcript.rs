//! A game's transcript: omniscient text for a human reader. Each turn opens with both
//! players' life, library size, hand and board (power and toughness, counters, tapped); every
//! decision the engine asked follows on one line, with who decided, its kind, the candidates
//! in short labels and the choice. What happened in between, as the engine's events tell it
//! (spells resolving or countered, creatures dying, damage, life gained), comes before the
//! decision it led to, on lines that start with `*`.

use std::collections::HashMap;
use std::fmt::Write;

use beryl_engine::card::{AbilityIx, CastingOption, FaceIx, Types};
use beryl_engine::event::{Event, Reason, Recipient};
use beryl_engine::state::{CounterKind, ObjectFlags, ObjectRef};
use beryl_engine::{
    Candidates, Choice, Decision, GameTable, Opt, Outcome, Seat, Slot, State, TurnStep, Zone,
};

pub struct Transcript {
    names: [String; 2],
    text: String,
    turn: Option<u16>,
    /// The name of each object seen at a decision, by slot, with its incarnation: events
    /// name objects by slot, and a token that has left the battlefield is gone by the time
    /// its events are written.
    seen: HashMap<Slot, (u16, String, Seat)>,
}

impl Transcript {
    /// A transcript of a game between the agents named for each seat.
    pub fn new(names: [&str; 2]) -> Transcript {
        Transcript {
            names: names.map(str::to_owned),
            text: String::new(),
            turn: None,
            seen: HashMap::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn who(&self, p: Seat) -> String {
        format!("P{} {}", p.get(), self.names[p.index()])
    }

    /// Records decision `d` and its answer `c`, after what `happened` since the previous
    /// decision, opening a new turn first if one began.
    pub fn decision(
        &mut self,
        t: &GameTable,
        s: &State,
        d: &Decision,
        c: Choice,
        happened: &[Event],
    ) {
        // What happened before a new turn's untap step belongs to the turn before.
        let split = happened
            .iter()
            .position(|e| {
                matches!(
                    e,
                    Event::StepBegins {
                        step: TurnStep::Untap,
                        ..
                    }
                )
            })
            .unwrap_or(happened.len());
        self.events(t, s, &happened[..split]);
        let turn = s.turn();
        if self.turn != Some(turn.number) {
            self.turn = Some(turn.number);
            self.board(t, s);
        }
        self.events(t, s, &happened[split..]);
        self.remember(t, s);
        let labels: Vec<String> = match &d.candidates {
            Candidates::Objects(b) => b.iter().map(|slot| candidate(t, s, slot)).collect(),
            Candidates::Options(o) => o.iter().map(|&o| option(t, s, o)).collect(),
        };
        let _ = writeln!(
            self.text,
            "  {:<9} {:<14} {}  -> {}",
            self.who(d.player),
            format!("{:?}", d.kind),
            labels.join(" | "),
            labels[usize::from(c.0)],
        );
    }

    /// Both players at the start of a turn.
    fn board(&mut self, t: &GameTable, s: &State) {
        let turn = s.turn();
        let _ = writeln!(
            self.text,
            "\n== Turn {}, {} active, {:?} ==",
            turn.number,
            self.who(turn.active),
            turn.step
        );
        for p in Seat::BOTH {
            let player = s.player(p);
            let hand: Vec<String> = s
                .zone(p, Zone::Hand)
                .iter()
                .map(|slot| name(t, s, slot))
                .collect();
            let board: Vec<String> = s
                .battlefield()
                .iter()
                .filter(|&&slot| s.controller(slot) == p)
                .map(|&slot| permanent(t, s, slot))
                .collect();
            let _ = writeln!(
                self.text,
                "{}: life {}, library {}, graveyard {}\n  hand: {}\n  board: {}",
                self.who(p),
                player.life,
                s.library(p).len(),
                s.graveyard(p).len(),
                list(&hand),
                list(&board),
            );
        }
    }

    /// Remembers the name and owner of every object on the battlefield or the stack.
    fn remember(&mut self, t: &GameTable, s: &State) {
        let stack = s.stack().iter().filter_map(|e| e.spell().map(|r| r.slot));
        for slot in s.battlefield().iter().copied().chain(stack) {
            let o = s.object(slot);
            self.seen
                .insert(slot, (o.incarnation, name(t, s, slot), o.owner));
        }
    }

    /// The name of an object an event refers to: as it is now if it is still that object,
    /// else as it was last seen, for one that has since changed zones or ceased to exist.
    fn called(&self, t: &GameTable, s: &State, r: ObjectRef) -> String {
        let o = s.object(r.slot);
        if o.zone != Zone::Nowhere && o.incarnation == r.incarnation {
            return name(t, s, r.slot);
        }
        match self.last_seen(r) {
            Some((n, _)) => n.clone(),
            None => name(t, s, r.slot),
        }
    }

    /// The owner of an object an event refers to, as for [`Transcript::called`]: a token
    /// that has ceased to exist has none now.
    fn owner(&self, s: &State, r: ObjectRef) -> Seat {
        let o = s.object(r.slot);
        if o.zone != Zone::Nowhere && o.incarnation == r.incarnation {
            return o.owner;
        }
        self.last_seen(r).map_or(o.owner, |(_, &owner)| owner)
    }

    /// How the object was last seen at a decision, if it was that object or the one it
    /// became by changing zones.
    fn last_seen(&self, r: ObjectRef) -> Option<(&String, &Seat)> {
        self.seen
            .get(&r.slot)
            .filter(|(incarnation, _, _)| {
                *incarnation == r.incarnation || incarnation.wrapping_add(1) == r.incarnation
            })
            .map(|(_, n, owner)| (n, owner))
    }

    /// One line for each event a reader needs to follow the game.
    fn events(&mut self, t: &GameTable, s: &State, happened: &[Event]) {
        for e in happened {
            let line = match *e {
                Event::ZoneChange {
                    object,
                    from,
                    to,
                    reason,
                } => zone_change(
                    &self.called(t, s, object),
                    self.owner(s, object),
                    from,
                    to,
                    reason,
                ),
                Event::Damage {
                    source, to, amount, ..
                } => {
                    let to = match to {
                        Recipient::Player(p) => format!("P{}", p.get()),
                        Recipient::Permanent(r) => self.called(t, s, r),
                    };
                    Some(format!(
                        "{} deals {amount} damage to {to}",
                        self.called(t, s, source)
                    ))
                }
                Event::LifeGain { player, amount } => {
                    Some(format!("P{} gains {amount} life", player.get()))
                }
                Event::Attack { player, attackers } => {
                    let names: Vec<String> = attackers
                        .iter()
                        .map(|a| self.called(t, s, s.object_ref(a)))
                        .collect();
                    Some(format!(
                        "P{} attacks with {}",
                        player.get(),
                        names.join(", ")
                    ))
                }
                Event::GainControl { object, player } => Some(format!(
                    "P{} gains control of {}",
                    player.get(),
                    self.called(t, s, object)
                )),
                Event::CoinFlip { player, won } => Some(format!(
                    "P{} {} a coin flip",
                    player.get(),
                    if won { "wins" } else { "loses" }
                )),
                _ => None,
            };
            if let Some(line) = line {
                let _ = writeln!(self.text, "    * {line}");
            }
        }
    }

    /// Records how the game ended.
    pub fn end(&mut self, t: &GameTable, s: &State, outcome: Outcome, happened: &[Event]) {
        self.events(t, s, happened);
        let result = match outcome {
            Outcome::Win(p) => format!("{} wins", self.who(p)),
            Outcome::Draw => "a draw".to_owned(),
            Outcome::Stalled(reason) => format!("stalled ({reason:?})"),
        };
        let _ = writeln!(
            self.text,
            "\n== {result} on turn {}; life {} to {} ==",
            s.turn().number,
            s.player(Seat::P0).life,
            s.player(Seat::P1).life
        );
    }
}

/// A zone change worth a line: spells resolving or countered, permanents leaving the
/// battlefield, tokens created, cards discarded or exiled. Draws, shuffles and lands played
/// are left out; the decisions and each turn's hands show them.
fn zone_change(n: &str, owner: Seat, from: Zone, to: Zone, reason: Reason) -> Option<String> {
    let owner = owner.get();
    Some(match (from, to, reason) {
        (_, _, Reason::Counter) => format!("{n} is countered"),
        (Zone::Stack, Zone::Battlefield, _) | (Zone::Stack, _, Reason::Resolve) => {
            format!("{n} resolves")
        }
        (Zone::Stack | Zone::Battlefield, Zone::Hand, _) => {
            format!("{n} is returned to P{owner}'s hand")
        }
        (_, Zone::Battlefield, Reason::Created) => format!("{n} is created"),
        (_, Zone::Battlefield, Reason::Return) => format!("{n} returns to the battlefield"),
        (Zone::Battlefield, Zone::Graveyard, Reason::Destroy) => format!("{n} is destroyed"),
        (Zone::Battlefield, Zone::Graveyard, Reason::Sacrifice) => format!("{n} is sacrificed"),
        (Zone::Battlefield, Zone::Graveyard, _) => format!("{n} dies"),
        (Zone::Battlefield, Zone::Library, _) => format!("{n} is put into P{owner}'s library"),
        (Zone::Battlefield, Zone::Exile, _) => format!("{n} is exiled"),
        (Zone::Hand, Zone::Graveyard, Reason::Discard) => format!("P{owner} discards {n}"),
        (Zone::Hand, Zone::Exile, _) => format!("{n} is exiled from P{owner}'s hand"),
        _ => return None,
    })
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_owned()
    } else {
        items.join(", ")
    }
}

/// The name of the card in a slot, by the face it has up; a face-down permanent, which has no
/// name, by its card's.
fn name(t: &GameTable, s: &State, slot: Slot) -> String {
    let def = t.def(s.card(slot));
    match s.object(slot).face {
        FaceIx::DOWN => format!("face-down {}", def.name),
        face => def.face_name(face).to_owned(),
    }
}

/// An object among a decision's candidates: a permanent or a spell with its controller, so
/// that a reader can tell whose it is.
fn candidate(t: &GameTable, s: &State, slot: Slot) -> String {
    let n = name(t, s, slot);
    match s.object(slot).zone {
        Zone::Battlefield | Zone::Stack => format!("{n} (P{})", s.controller(slot).get()),
        _ => n,
    }
}

/// A permanent with its power and toughness, counters and status.
fn permanent(t: &GameTable, s: &State, slot: Slot) -> String {
    let o = s.object(slot);
    let d = s.derived(slot);
    let mut text = name(t, s, slot);
    if d.types.intersects(Types::CREATURE) {
        let _ = write!(text, " {}/{}", d.power, d.toughness);
    }
    let c = o.counters;
    for (kind, n) in [
        (CounterKind::PlusOne, c.plus_one),
        (CounterKind::MinusOne, c.minus_one),
        (CounterKind::Loyalty, c.loyalty),
        (c.other_kind, c.other),
    ] {
        if n > 0 {
            let _ = write!(text, " [{n} {}]", counter(kind));
        }
    }
    if o.damage > 0 {
        let _ = write!(text, " ({} damage)", o.damage);
    }
    if o.flags.has(ObjectFlags::TAPPED) {
        text.push_str(" (tapped)");
    }
    if let Some(host) = o.attached_to {
        let _ = write!(text, " (on {})", name(t, s, host));
    }
    text
}

fn counter(kind: CounterKind) -> &'static str {
    match kind {
        CounterKind::PlusOne => "+1/+1",
        CounterKind::MinusOne => "-1/-1",
        CounterKind::Loyalty => "loyalty",
        CounterKind::Time => "time",
        CounterKind::Story => "story",
        CounterKind::Chorus => "chorus",
    }
}

/// A short label for one option.
fn option(t: &GameTable, s: &State, o: Opt) -> String {
    let named = |slot| name(t, s, slot);
    match o {
        Opt::Pass => "pass".to_owned(),
        Opt::PlayLand(slot) => format!("play {}", named(slot)),
        Opt::Play => "play first".to_owned(),
        Opt::Draw => "draw first".to_owned(),
        Opt::Keep => "keep".to_owned(),
        Opt::Mulligan => "mulligan".to_owned(),
        Opt::Trigger(i) => {
            let trigger = s.triggers()[usize::from(i)];
            format!("{} trigger", t.def(trigger.card).face_name(trigger.face))
        }
        Opt::Yes => "yes".to_owned(),
        Opt::No => "no".to_owned(),
        Opt::Done => "done".to_owned(),
        Opt::Attack(slot) => format!("attack with {}", named(slot)),
        Opt::Defender(None) => "the player".to_owned(),
        Opt::Defender(Some(slot)) | Opt::Object(slot) => named(slot),
        Opt::Block(slot) => format!("block with {}", named(slot)),
        Opt::Lethal(slot) => format!("lethal to {}", named(slot)),
        Opt::Cast { slot, face, costs } => {
            let def = t.def(s.card(slot));
            let mut text = format!("cast {}", def.face_name(face));
            for option in costs.paid(def.face(face)) {
                text.push_str(match option {
                    CastingOption::Kicker { .. } => " kicked",
                    CastingOption::Offspring(_) => " with offspring",
                    CastingOption::Impending { .. } => " for impending",
                    CastingOption::Overload { .. } => " overloaded",
                    CastingOption::Evoke { .. } => " for evoke",
                });
            }
            text
        }
        Opt::Activate { slot, ability } => {
            let def = t.def(s.card(slot));
            match ability {
                AbilityIx::Own(i) => {
                    let face = def.face(s.object(slot).face);
                    let a = &face.abilities[usize::from(i)];
                    match a.cost.loyalty {
                        Some(l) => format!("{} {l:+}", named(slot)),
                        None => format!("activate {} #{i}", named(slot)),
                    }
                }
                AbilityIx::Granted(k) => format!("activate {} granted #{k}", named(slot)),
            }
        }
        Opt::Mana { slot, color } => format!("tap {} for {}", named(slot), color.index()),
        Opt::Pool(color) => format!("spend pool {}", color.index()),
        Opt::Number(n) => n.to_string(),
        Opt::Plan(i) => format!("plan {}", i + 1),
        Opt::Player(p) => format!("P{}", p.get()),
        Opt::TurnFaceUp(slot) => format!("turn {} face up", named(slot)),
    }
}
