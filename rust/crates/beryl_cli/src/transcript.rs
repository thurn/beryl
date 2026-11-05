//! A game's transcript: omniscient text for a human reader. Each turn opens with both
//! players' life, library size, hand and board (power and toughness, counters, tapped); every
//! decision the engine asked follows on one line, with who decided, its kind, the candidates
//! in short labels and the choice.

use std::fmt::Write;

use beryl_engine::card::{AbilityIx, CastingOption, Types};
use beryl_engine::state::{CounterKind, ObjectFlags};
use beryl_engine::{
    Candidates, Choice, Decision, GameTable, Opt, Outcome, Seat, Slot, State, Zone,
};

pub struct Transcript {
    names: [String; 2],
    text: String,
    turn: Option<u16>,
}

impl Transcript {
    /// A transcript of a game between the agents named for each seat.
    pub fn new(names: [&str; 2]) -> Transcript {
        Transcript {
            names: names.map(str::to_owned),
            text: String::new(),
            turn: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn who(&self, p: Seat) -> String {
        format!("P{} {}", p.get(), self.names[p.index()])
    }

    /// Records decision `d` and its answer `c`, opening a new turn first if one began.
    pub fn decision(&mut self, t: &GameTable, s: &State, d: &Decision, c: Choice) {
        let turn = s.turn();
        if self.turn != Some(turn.number) {
            self.turn = Some(turn.number);
            self.board(t, s);
        }
        let labels: Vec<String> = match &d.candidates {
            Candidates::Objects(b) => b.iter().map(|slot| name(t, s, slot)).collect(),
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

    /// Records how the game ended.
    pub fn end(&mut self, s: &State, outcome: Outcome) {
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

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_owned()
    } else {
        items.join(", ")
    }
}

/// The name of the card in a slot, by the face it has up.
fn name(t: &GameTable, s: &State, slot: Slot) -> String {
    t.def(s.card(slot))
        .face_name(s.object(slot).face)
        .to_owned()
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
            for (i, option) in def.faces[usize::from(face)].casting.iter().enumerate() {
                if costs & (1 << i) != 0 {
                    text.push_str(match option {
                        CastingOption::Kicker { .. } => " kicked",
                        CastingOption::Offspring(_) => " with offspring",
                        CastingOption::Impending { .. } => " for impending",
                    });
                }
            }
            text
        }
        Opt::Activate { slot, ability } => {
            let def = t.def(s.card(slot));
            match ability {
                AbilityIx::Own(i) => {
                    let face = &def.faces[usize::from(s.object(slot).face)];
                    let a = &face.abilities[usize::from(i)];
                    match a.cost.loyalty {
                        Some(l) => format!("{} {l:+}", named(slot)),
                        None => format!("activate {} #{i}", named(slot)),
                    }
                }
                AbilityIx::Granted(k) => format!("activate {} granted #{k}", named(slot)),
            }
        }
        Opt::Mana { slot, color } => format!("tap {} for {color}", named(slot)),
        Opt::Pool(color) => format!("spend pool {color}"),
        Opt::Number(n) => n.to_string(),
        Opt::Plan(i) => format!("plan {}", i + 1),
        Opt::Player(p) => format!("P{}", p.get()),
    }
}
