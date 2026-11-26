//! The sounds of what happens in a game: each step of a script that has a sound asks for it at
//! the start or the end of the step, and the show lists it in the frame that first reaches that
//! time (docs/beryl-3d.md, "Sound"). This is a port of `CUES` in `ui/src/audio/steps.ts`, so the
//! two clients play the same sounds for the same steps. Nothing here plays a sound.

use beryl_protocol::protocol::{ObjectView, PlayerView, Side, StackItem, Why, ZoneName};
use beryl_protocol::script::{
    Act, Edge, Fact, Notice, Script, Step, StepTiming, Stream, Subject, Table as Played, Tally,
};

use crate::beat::Laid;
use crate::play::nanoseconds;
use kit_frame::frame::{Sound, SoundId};

include!(concat!(env!("OUT_DIR"), "/sounds.rs"));

/// A sound a step asks for, the edge of the step it is asked for at, and how far into the step
/// after that edge it sounds, as a share of the step's time (none: at the edge).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub sound: SoundId,
    pub edge: Edge,
    pub into: Option<f64>,
}

/// What a step's sounds are picked from besides its act: the table as the step found it, the
/// steps of its beat before it, and the timing it plays with.
#[derive(Clone, Copy, Debug)]
pub struct Found<'a> {
    pub table: &'a Played,
    pub earlier: &'a [Step],
    pub timing: &'a StepTiming,
}

/// A sound at a time on its script's clock, in nanoseconds. `step` is the number of the step
/// that asked for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundRun {
    pub sound: Sound,
    pub at: u64,
    pub step: usize,
}

/// The id of the sound the manifest names `name`.
///
/// # Panics
///
/// If the manifest has no such sound.
#[must_use]
pub fn id(name: &str) -> SoundId {
    let at = NAMES
        .iter()
        .position(|other| *other == name)
        .unwrap_or_else(|| panic!("the manifest has no sound {name}"));
    SoundId(u16::try_from(at).expect("the sounds' ids fit 16 bits"))
}

/// The name the manifest gives the sound `id`, the name the web interface's `Cue` carries.
#[must_use]
pub fn name(id: SoundId) -> &'static str {
    NAMES[usize::from(id.0)]
}

fn at(edge: Edge, sound: Option<&str>) -> [Option<Cue>; 2] {
    [
        sound.map(|name| Cue {
            sound: id(name),
            edge,
            into: None,
        }),
        None,
    ]
}

/// The sound of a card arriving in `zone`.
fn arriving(zone: ZoneName) -> Option<&'static str> {
    match zone {
        ZoneName::Battlefield => Some("place"),
        ZoneName::Graveyard => Some("shove"),
        ZoneName::Exile => Some("exile"),
        ZoneName::Hand => Some("slide"),
        ZoneName::Library => Some("fan"),
        // A spell sounds by its cast, which comes with its move to the stack; a token ceasing to
        // exist, once its move to a graveyard has sounded; a card merging with a permanent, by
        // its spell's resolution.
        ZoneName::Sideboard | ZoneName::Stack | ZoneName::Nowhere | ZoneName::Merged => None,
    }
}

/// A card going from `from` to `to`, as it sets out.
fn moving(from: ZoneName, to: ZoneName, why: Why) -> Option<&'static str> {
    // A spell sounds as it is cast, however it comes to the stack.
    if to == ZoneName::Stack {
        return (why == Why::Cast).then_some("cast");
    }
    if from == ZoneName::Stack && to != ZoneName::Battlefield {
        // A spell done resolving chimes; one taken off the stack otherwise (countered, bounced,
        // exiled) is struck.
        if why == Why::Resolve {
            return Some("resolve");
        }
        if why != Why::Cast && why != Why::Play {
            return Some("shatter");
        }
    }
    if from == to { None } else { arriving(to) }
}

fn record_of(table: &Played, slot: beryl_protocol::protocol::ObjectId) -> Option<&ObjectView> {
    table.objects.get(&slot)
}

/// How many counters of `kind` the object has.
fn counted(object: Option<&ObjectView>, kind: beryl_protocol::protocol::Counter) -> i64 {
    object.map_or(0, |object| {
        object
            .counters
            .iter()
            .find(|counter| counter.kind == kind)
            .map_or(0, |counter| i64::from(counter.count))
    })
}

/// Whether `player` is dealt damage by a step of `steps`, whose sound says the life it costs.
fn hurt(steps: &[Step], player: Side) -> bool {
    steps.iter().any(|step| match &step.act {
        Act::Strike { to, .. } => *to == Subject::Player { player },
        Act::Streak { stream, to, .. } => {
            *stream == Stream::Damage && *to == Subject::Player { player }
        }
        _ => false,
    })
}

/// A permanent newly declared an attacker, as its record says.
fn attacks(object: &ObjectView, table: &Played) -> bool {
    use beryl_protocol::protocol::Combat;
    matches!(object.combat, Some(Combat::Attacking { .. }))
        && !matches!(
            record_of(table, object.slot).and_then(|record| record.combat.as_ref()),
            Some(Combat::Attacking { .. })
        )
}

/// The sounds the step with `act` asks for, given what it found.
pub fn cues(act: &Act, found: &Found<'_>) -> impl Iterator<Item = Cue> {
    pair(act, found).into_iter().flatten()
}

/// The sounds the step with `act` asks for, at most two, one match over the verbs with no
/// wildcard arm, so that a new verb does not build until it has a line.
#[allow(clippy::too_many_lines)] // One arm for each verb.
fn pair(act: &Act, found: &Found<'_>) -> [Option<Cue>; 2] {
    let table = found.table;
    match act {
        // A card is placed as it lands; any other move sounds as it sets out. A permanent that
        // stays on the battlefield and changes sides is taken.
        Act::Fly {
            from, why, object, ..
        } => {
            let taken = *from == object.zone
                && record_of(table, object.slot).map(|record| record.controller)
                    != Some(object.controller);
            let sound = if taken {
                Some("control")
            } else {
                moving(*from, object.zone, *why)
            };
            at(
                if sound == Some("place") {
                    Edge::End
                } else {
                    Edge::Start
                },
                sound,
            )
        }
        // No flight: a token is placed as it appears.
        Act::Appear {
            from, why, object, ..
        } => at(Edge::Start, moving(*from, object.zone, *why)),
        // A card is then at its new place; an ability done, or a token ceasing to exist, has none.
        Act::Vanish {
            from, why, object, ..
        } => at(
            Edge::Start,
            object
                .as_ref()
                .and_then(|object| moving(*from, object.zone, *why)),
        ),
        Act::Emerge { .. } => at(Edge::Start, Some("ability")),
        // An attacker taps as it is declared: its record says both.
        Act::Tap { object } => [
            Some(Cue {
                sound: id(if object.tapped { "tap" } else { "untap" }),
                edge: Edge::Start,
                into: None,
            }),
            attacks(object, table).then(|| Cue {
                sound: id("attack"),
                edge: Edge::Start,
                into: None,
            }),
        ],
        // A creature hits as its lunge lands, a share of the way through its strike (the `hit`
        // of the phases its timing sends), where Arena's screen flashes: one step, so the sound
        // waits within it.
        Act::Strike { .. } => [
            Some(Cue {
                sound: id("hit"),
                edge: Edge::Start,
                into: found
                    .timing
                    .phases
                    .iter()
                    .find(|phase| phase.name == "hit")
                    .map(|phase| phase.at),
            }),
            None,
        ],
        // Mana sounds as it leaves its source; damage hits as its streak lands.
        Act::Streak { stream, .. } => match stream {
            Stream::Mana { .. } => at(Edge::Start, Some("mana")),
            Stream::Damage => at(Edge::End, Some("hit")),
        },
        Act::Count { tally } => count(tally, found),
        Act::Set { fact } => match fact {
            // A step ticks as it begins. A new turn rings as its banner shows (`announce`).
            Fact::Turn { number, step, .. } => {
                let turn = &table.turn;
                at(
                    Edge::Start,
                    (*number == turn.number && *step != turn.step).then_some("step"),
                )
            }
            Fact::Object { object } => at(Edge::Start, attacks(object, table).then_some("attack")),
            // A library is shuffled.
            Fact::Library { .. } | Fact::ShuffledLibrary { .. } => at(Edge::Start, Some("fan")),
            Fact::Forget { .. } | Fact::Item { .. } => [None, None],
        },
        Act::Announce { notice } => at(
            Edge::Start,
            match notice {
                Notice::Turn { .. } | Notice::TableTurn { .. } => Some("turn"),
                Notice::Coin { .. } | Notice::Roll { .. } => Some("coin"),
                Notice::Phase { .. } => Some("step"),
                Notice::Shuffle
                | Notice::Rewind { .. }
                | Notice::Game { .. }
                | Notice::Scry { .. }
                | Notice::TopOrBottom { .. }
                | Notice::Chosen { .. } => None,
            },
        ),
        Act::Flip { .. }
        | Act::Showcase { .. }
        | Act::Dwell { .. }
        | Act::Point { .. }
        | Act::Mark { .. }
        | Act::Lift { .. } => [None, None],
    }
}

/// The sounds of a `count`, from the tally it sets and the table it found.
fn count(tally: &Tally, found: &Found<'_>) -> [Option<Cue>; 2] {
    let table = found.table;
    match tally {
        Tally::Life { player, life } => {
            let was = table.player(*player).life;
            if *life > was {
                return at(Edge::Start, Some("gain"));
            }
            // Life lost to damage is the hit's to say.
            let drained = *life < was && !hurt(found.earlier, *player);
            at(Edge::Start, drained.then_some("drain"))
        }
        // Mana that stays in a pool; what leaves one streaks.
        Tally::Pool { player, mana } => {
            let sum = |pool: &beryl_protocol::protocol::ManaPool| {
                [
                    pool.white,
                    pool.blue,
                    pool.black,
                    pool.red,
                    pool.green,
                    pool.colorless,
                ]
                .iter()
                .map(|n| i64::from(*n))
                .sum::<i64>()
            };
            at(
                Edge::Start,
                (sum(mana) > sum(&table.player(*player).mana)).then_some("mana"),
            )
        }
        Tally::Counters { counter, object } => {
            let now = counted(Some(object), *counter);
            let was = counted(record_of(table, object.slot), *counter);
            at(
                Edge::Start,
                match now.cmp(&was) {
                    std::cmp::Ordering::Greater => Some("chip"),
                    std::cmp::Ordering::Less => Some("chips"),
                    std::cmp::Ordering::Equal => None,
                },
            )
        }
        Tally::PlayerCounters { .. }
        | Tally::NamedCounters { .. }
        | Tally::Damage { .. }
        | Tally::Stats { .. } => [None, None],
    }
}

/// Who controls what `subject` names on `table`.
fn controller_of(table: &Played, subject: Subject) -> Option<Side> {
    match subject {
        Subject::Player { player } => Some(player),
        Subject::Object { slot } => record_of(table, slot).map(|record| record.controller),
        Subject::Ability { id } => table.stack.iter().find_map(|entry| match &entry.item {
            StackItem::Ability { controller, .. } if entry.id == Some(id) => Some(*controller),
            _ => None,
        }),
    }
}

/// Whose doing `step` is: its cause's controller, or else whose the record it carries is.
#[must_use]
pub fn side_of(step: &Step, table: &Played) -> Option<Side> {
    let act = &step.act;
    if let Act::Emerge {
        item: StackItem::Ability { controller, .. },
        ..
    } = act
    {
        return Some(*controller);
    }
    if let Some(cause) = step.cause {
        return controller_of(table, cause);
    }
    let carried = match act {
        Act::Fly { object, .. }
        | Act::Appear { object, .. }
        | Act::Flip { object }
        | Act::Tap { object }
        | Act::Showcase { object } => Some(object),
        Act::Vanish { object, .. } | Act::Strike { object, .. } | Act::Streak { object, .. } => {
            object.as_ref()
        }
        _ => None,
    };
    if let Some(object) = carried {
        return Some(object.controller);
    }
    match act {
        Act::Count { tally } => match tally {
            Tally::Life { player, .. }
            | Tally::Pool { player, .. }
            | Tally::PlayerCounters { player, .. } => Some(*player),
            Tally::Counters { object, .. }
            | Tally::NamedCounters { object, .. }
            | Tally::Damage { object }
            | Tally::Stats { object } => Some(object.controller),
        },
        Act::Set {
            fact: Fact::Turn { active, .. },
        } => Some(*active),
        Act::Announce { notice } => match notice {
            Notice::Turn { active } => Some(*active),
            Notice::Scry { player, .. }
            | Notice::TopOrBottom { player, .. }
            | Notice::Chosen { player, .. }
            | Notice::Coin { player, .. } => Some(*player),
            _ => None,
        },
        _ => None,
    }
}

/// The sounds of a script's steps, collected as it is compiled.
pub(crate) struct Sounds {
    /// The table the steps find where the script is not played over a table: the table the
    /// script leads to, made only if a step needs it.
    unplayed: Option<Played>,
    /// Each sound with the time of the edge it was asked for at, which is what two of the same
    /// sound at one time are told apart by.
    asked: Vec<(u64, SoundRun)>,
}

impl Sounds {
    /// The sounds of `script`, which leads to the view `after` and is played over a table
    /// unless `played` is false.
    pub(crate) fn new(script: &Script, after: &PlayerView, played: bool) -> Self {
        let stepped = script.beats.iter().any(|beat| !beat.is_empty());
        Self {
            unplayed: (!played && stepped).then(|| Played::of(after)),
            asked: Vec::new(),
        }
    }

    /// Adds the sounds of the step `index` of `beat`, the script's step `number`, which plays as
    /// `laid` says, over `table`, the table as the step finds it before its act is played over
    /// it, if the script is played over one. Returns the cues the step asked for, which a
    /// script's record keeps for `just client timeline`.
    pub(crate) fn step(
        &mut self,
        table: Option<&Played>,
        (beat, index, number): (&[Step], usize, usize),
        timing: &StepTiming,
        laid: &Laid,
    ) -> [Option<Cue>; 2] {
        let table = table
            .or(self.unplayed.as_ref())
            .expect("a step of a script finds some table");
        let step = &beat[index];
        let found = Found {
            table,
            earlier: &beat[..index],
            timing,
        };
        let theirs = side_of(step, table) == Some(Side::Opponent);
        let span = laid.to - laid.from;
        let asked = pair(&step.act, &found);
        for cue in asked.into_iter().flatten() {
            let edge = match cue.edge {
                Edge::Start => laid.from,
                Edge::End => laid.to,
            };
            self.asked.push((
                nanoseconds(edge),
                SoundRun {
                    sound: Sound {
                        id: cue.sound,
                        theirs,
                    },
                    at: nanoseconds(edge + cue.into.unwrap_or(0.0) * span),
                    step: number,
                },
            ));
        }
        asked
    }

    /// The sounds of the steps so far in the order they begin, a sound asked for more than once
    /// at one time once, as the first of those steps asks.
    pub(crate) fn runs(&self) -> Vec<SoundRun> {
        let mut asked = self.asked.clone();
        asked.sort_by_key(|(edge, _)| *edge);
        let mut kept = 0;
        for next in 0..asked.len() {
            let (edge, run) = asked[next];
            let again = asked[..kept]
                .iter()
                .rev()
                .take_while(|(other, _)| *other == edge)
                .any(|(_, other)| other.sound.id == run.sound.id);
            if !again {
                asked[kept] = (edge, run);
                kept += 1;
            }
        }
        asked.truncate(kept);
        let mut runs = Vec::with_capacity(kept);
        runs.extend(asked.iter().map(|(_, run)| *run));
        runs.sort_by_key(|run| run.at);
        runs
    }
}

/// The music that plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Music {
    /// The menu's music, everywhere but in a game and in the gallery.
    Menu,
    /// The game's music, while a game goes on.
    Game,
    /// The sting for a game the viewer won.
    Win,
    /// The sting for a game the viewer did not win.
    Loss,
}
