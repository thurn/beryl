//! How a step is played: the records the session sends beside a script so that both clients
//! time it the same way, and the rules that fill them in. The records are the web interface's
//! `Timing` and `Effect` records (`ui/src/stage/tracks.ts`) with the same meanings. These
//! rules are the only place a step's case, waits and effects are chosen: both clients play
//! the timing they are sent. Times are in milliseconds.
//!
//! The rules read the design language's data (`data/ui-conventions.json`, `verbs` and
//! `effects`): a verb's duration, delay and stagger and its curve, a case's own where the
//! verb has several, and the effects each verb's steps begin.

mod conventions;
mod own;
#[cfg(test)]
mod rules;
#[cfg(test)]
mod tests;

pub use own::{OwnStep, OwnTiming, OwnTimings, own_timings};

use std::ptr;

use serde::{Deserialize, Serialize};

use self::conventions::{Conventions, curve, snap};
use super::{
    Act, Arrow, Fact, Mark, Notice, Script, Span, Stream, Subject, Table, Tally, Verb, replay,
};
use crate::protocol::{
    CardHandling, Counter, ObjectId, ObjectView, Side, StackItem, Step as TurnStep, Why, ZoneName,
};

/// How one step is played. Times are in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)] // Each flag is one independent rule of the wire format.
pub struct StepTiming {
    /// The verb's case in `data/ui-conventions.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<String>,
    /// How long its motion takes.
    pub duration: f64,
    /// How long after its turn comes it begins.
    pub delay: f64,
    /// How long the next step waits for its turn.
    pub stagger: f64,
    /// The easing of its motion.
    pub curve: Curve,
    /// For a step that moves several cards one after another, how long each one moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub each: Option<f64>,
    /// It takes the turn of the step before it.
    pub with: bool,
    /// It waits until the steps of its beat that move these have ended.
    pub after: Vec<Subject>,
    /// It ends a span, and waits until the span has stood whole this long.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<f64>,
    /// What follows it waits until it is over.
    pub leads: bool,
    /// Its beat does not wait for it.
    pub passes: bool,
    /// It waits until nothing passes over the table.
    pub alone: bool,
    /// Its act is applied as its delay begins.
    pub opens: bool,
    /// Its act is applied with the step before's.
    pub ahead: bool,
    /// How long what it left room for takes to close up after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closes: Option<f64>,
    /// How what it makes room among (the cards whose resting place its act changed but that
    /// it does not move itself) goes to its new place. Where `opens` or `closes` is set, it
    /// says when that room is made instead, except for the cards of a hand that regroup in the
    /// time [`Room::For`] gives.
    #[serde(default)]
    pub room: Room,
    /// For a verb whose motion has named phases (a strike's poise and hit), where each phase
    /// begins, as a share of the step, in the order the motion reaches them.
    #[serde(default)]
    pub phases: Vec<Phase>,
    /// The effects it begins.
    pub effects: Vec<StepEffect>,
}

/// How what a step makes room among goes to its new place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Room {
    /// Over the step.
    #[default]
    Step,
    /// The cards that stay in a hand as a card joins or leaves it, or as a prompt holds some
    /// of them up, go to their new places in `ms`, easing out, from when the step's act is
    /// applied, whatever the step's own time (`regroup` in the data). Whatever else the step
    /// makes room among does so over the step.
    For { ms: f64 },
    /// From `share` of the way through the step to its end: a permanent that taps turns where
    /// it lies, and only then does its row make room (the `tap` verb's `room` in the data).
    From { share: f64 },
}

/// One named phase of a step's motion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phase {
    /// The phase's name in the data (`poise`, `hit`).
    pub name: String,
    /// Where it begins, as a share of the step from 0 to 1.
    pub at: f64,
}

/// An effect a step begins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepEffect {
    /// The effect's name in the data.
    pub name: String,
    /// The edge of the step it begins at.
    pub when: Edge,
    /// What it is on, where that is not the step's own subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<Subject>,
    /// How far through the step it begins, from 0 to 1, where that is not an edge of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<f64>,
    /// How long it lasts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    /// How long after it begins it starts to show.
    pub delay: f64,
    /// The easing of the effect.
    pub curve: Curve,
}

/// The easing of a motion or an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Curve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

/// An end of a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Start,
    End,
}

/// Arena's name for the case the step `act` is of (`data/arena-motion.json`'s `case`): none
/// for a case Arena's instances do not have, which is played with its verb's values. `found`
/// is the table the step finds, which says whether a life total rises or falls, and `beside`
/// the acts of its whole beat, which say whether a card is drawn alone.
#[allow(clippy::too_many_lines)] // One branch for each verb.
pub fn case_of(act: &Act, found: &Table, beside: &[&Act]) -> Option<&'static str> {
    match act {
        Act::Tap { object } => Some(if object.tapped { "tap" } else { "untap" }),
        Act::Strike { to, .. } => Some(if matches!(to, Subject::Player { .. }) {
            "unblocked attacker"
        } else {
            "blocked attacker"
        }),
        Act::Count { tally } => match tally {
            Tally::Life { player, life } => {
                let was = found.player(*player).life;
                match life.cmp(&was) {
                    std::cmp::Ordering::Equal => None,
                    std::cmp::Ordering::Greater => Some("life gain"),
                    std::cmp::Ordering::Less => Some("life loss"),
                }
            }
            Tally::Counters { counter, .. } => Some(if *counter == Counter::Loyalty {
                "loyalty"
            } else {
                "counter"
            }),
            _ => None,
        },
        Act::Fly { from, object, .. } => match (from, object.zone) {
            (ZoneName::Hand, ZoneName::Stack) => Some(if object.controller == Side::You {
                "own spell cast"
            } else {
                "opponent spell cast"
            }),
            (ZoneName::Exile, ZoneName::Stack) => Some("exile to stack"),
            (ZoneName::Stack, ZoneName::Battlefield) => Some("permanent resolves"),
            (ZoneName::Hand, ZoneName::Battlefield) => Some(if object.controller == Side::You {
                "land played"
            } else {
                "opponent land played"
            }),
            (ZoneName::Library, ZoneName::Battlefield) => Some(if object.owner == Side::You {
                "library to battlefield"
            } else {
                "opponent library to battlefield"
            }),
            (ZoneName::Exile, ZoneName::Battlefield) => Some("exile to battlefield"),
            (ZoneName::Hand, ZoneName::Graveyard) => Some("discard"),
            // Yours, drawn alone during the beginning phase, plays at Arena's slower turn-draw
            // speed (0.7x); draws outside the beginning phase and multi-draws play at full speed
            // (1.0x), and cards no one reads fly face down (1.2x).
            (ZoneName::Library, ZoneName::Hand) => {
                if !draws(act) {
                    None
                } else if !draws_to_read(act) {
                    Some("opponent draw")
                } else if beside
                    .iter()
                    .any(|other| !ptr::eq(*other, act) && draws_to_read(other))
                {
                    Some("draw of several")
                } else if matches!(
                    found.turn.step,
                    TurnStep::Untap | TurnStep::Upkeep | TurnStep::Draw
                ) {
                    Some("draw")
                } else {
                    Some("fast draw")
                }
            }
            _ => None,
        },
        Act::Appear { from, why, .. } => {
            if *from == ZoneName::Nowhere {
                Some("token")
            } else if *why == Why::Draw {
                Some("opening hand")
            } else {
                None
            }
        }
        Act::Emerge { item, .. } => Some(
            if matches!(
                item,
                StackItem::Ability {
                    triggered: true,
                    ..
                }
            ) {
                "triggered ability"
            } else {
                "activated ability"
            },
        ),
        Act::Vanish {
            subject,
            from,
            why,
            object,
            ..
        } => {
            if matches!(subject, Subject::Ability { .. }) {
                return Some("ability finished");
            }
            // Where it goes says more than why. A permanent an effect returns to a hand is
            // bounced. A spell that leaves the stack for a hand by vanishing is the opponent's
            // (the viewer's own flies to their hand), and it breaks up in place on the stack as
            // a countered spell does, whether it was countered (Remand) or only returned
            // (Unsubstantiate) (UI Conventions, "An opponent's spell returned to hand": "It
            // breaks up in place on the stack over 0.5 to 1 s").
            match object.as_ref().map(|o| o.zone) {
                Some(ZoneName::Hand) if *from == ZoneName::Stack => return Some("countered"),
                Some(ZoneName::Hand) => return Some("bounced"),
                Some(ZoneName::Exile) => return Some("exiled"),
                _ => {}
            }
            match why {
                Why::Destroy | Why::StateBased => Some("destroyed"),
                Why::Counter => Some("countered"),
                Why::Resolve => Some("resolved instant or sorcery"),
                Why::Sacrifice => Some("sacrificed"),
                _ => None,
            }
        }
        Act::Point {
            span, arrow, to, ..
        } => {
            if *span == Span::End {
                return Some(if *arrow == Arrow::Source {
                    "source line end"
                } else {
                    "end"
                });
            }
            match arrow {
                Arrow::Source => Some("source line"),
                Arrow::Attack => Some("attack line"),
                Arrow::Block => Some("block line"),
                Arrow::FriendlyTarget | Arrow::HostileTarget => {
                    // At another spell or ability on the stack, its arrow loops over the two
                    // (UI Conventions, "Two items on the stack").
                    let stacked = match to {
                        Subject::Ability { .. } => true,
                        Subject::Object { slot } => found.stack.iter().any(|entry| {
                            let spell = match entry.item {
                                StackItem::Spell { slot, .. } => Some(slot),
                                StackItem::Ability { .. } => None,
                            };
                            spell == Some(*slot)
                        }),
                        Subject::Player { .. } => false,
                    };
                    Some(if stacked {
                        "stack arrow"
                    } else {
                        "target arrow"
                    })
                }
            }
        }
        Act::Mark { span, mark, .. } => {
            if *span == Span::End {
                return Some("end");
            }
            Some(match mark {
                Mark::FriendlyTarget | Mark::HostileTarget => "target",
                other => other.name(),
            })
        }
        Act::Streak { stream, .. } => Some(match stream {
            Stream::Mana { .. } => "mana",
            Stream::Damage => "damage",
        }),
        Act::Announce { notice } => match notice {
            Notice::Turn { .. } => Some("turn banner"),
            // The pill naming the end of a library the opponent chose stands as long as a
            // scry's.
            Notice::Scry { .. } | Notice::TopOrBottom { .. } => Some("scry pill"),
            Notice::Coin { .. } => Some("coin"),
            _ => None,
        },
        // On a player, it is the table left to be seen as the game ends (script/spans.rs).
        Act::Dwell { subject } => matches!(subject, Subject::Player { .. }).then_some("game's end"),
        Act::Set { fact } => match fact {
            Fact::Turn { .. } => Some("step begins"),
            Fact::Library { .. } | Fact::ShuffledLibrary { .. } => Some("library shuffled"),
            _ => None,
        },
        // The session's lifts have no tray: only the web stage's own steps of the cards a
        // prompt holds up are cases of `lift`, and those are not timed here.
        Act::Lift { .. } | Act::Flip { .. } | Act::Showcase { .. } => None,
    }
}

/// The data's timing of the step `act`, which finds the table `found`: its case's, or its
/// verb's, changed by the rules below, and the case it is of. `before` is the act of the step
/// before it in its beat, and `beside` the acts of the whole beat. Its effects are left empty:
/// [`effects_of`] gives them.
pub fn timing_of(act: &Act, found: &Table, before: Option<&Act>, beside: &[&Act]) -> StepTiming {
    let case = case_of(act, found, beside);
    StepTiming {
        case: case.map(str::to_owned),
        room: room_of(act, found),
        phases: phases_of(act),
        ..motion(act, found, case, before, beside)
    }
}

/// How what the step `act` makes room among goes to its new place. A permanent's row makes
/// room once it has turned, if it taps or untaps. The cards that stay in a hand go to their
/// new places in a sixth of a second if `act` takes a card into or out of a hand or the
/// opponent handles a hand: Arena's regroup in that time whatever the card's flight takes
/// (`SswBO8aOOgc 20:28.35` to `20:28.53` for a draw, `20:11.68` to `20:11.85` for a cast), and
/// moving them over the whole of a draw's flight read as a slow reshape of the hand. A card
/// that vanishes from a hand leaves its room to close once it has gone, as everything does
/// after a `vanish`.
fn room_of(act: &Act, found: &Table) -> Room {
    let data = Conventions::get();
    match act {
        Act::Vanish { .. } => Room::Step,
        Act::Tap { .. } => Room::From {
            share: data.turned(),
        },
        _ if touches_hand(act, found) => Room::For { ms: data.regroup() },
        _ => Room::Step,
    }
}

/// Whether `act` takes a card into or out of a hand, moves one within it, or has the opponent
/// handle a hand's cards: whether the other cards of a hand may move to new places.
fn touches_hand(act: &Act, found: &Table) -> bool {
    let in_hand = |slot: ObjectId| {
        found
            .objects
            .get(&slot)
            .is_some_and(|object| object.zone == ZoneName::Hand)
    };
    let record = |object: &ObjectView| object.zone == ZoneName::Hand || in_hand(object.slot);
    match act {
        Act::Fly { from, object, .. } | Act::Appear { from, object, .. } => {
            *from == ZoneName::Hand || record(object)
        }
        Act::Lift {
            handling: CardHandling::Hand { .. },
            ..
        } => true,
        Act::Set {
            fact: Fact::Object { object },
        } => record(object),
        Act::Set {
            fact: Fact::Forget { slots },
        } => slots.iter().copied().any(in_hand),
        _ => carried(act).is_some_and(record),
    }
}

/// Where each named phase of the step `act`'s motion begins: a strike's poise and hit.
fn phases_of(act: &Act) -> Vec<Phase> {
    if !matches!(act, Act::Strike { .. }) {
        return Vec::new();
    }
    let strike = Conventions::get().strike();
    vec![
        Phase {
            name: "poise".to_owned(),
            at: strike.poise,
        },
        Phase {
            name: "hit".to_owned(),
            at: strike.hit,
        },
    ]
}

/// [`timing_of`] but for its case.
#[allow(clippy::too_many_lines)] // One rule after another.
fn motion(
    act: &Act,
    found: &Table,
    case: Option<&'static str>,
    before: Option<&Act>,
    beside: &[&Act],
) -> StepTiming {
    let data = Conventions::get();
    // An ability that has finished is no verb's median: it is removed at once (UI Conventions,
    // "A finished ability"; Arena's is gone between two frames), and what it left room for
    // then closes up.
    if let Act::Vanish {
        subject: Subject::Ability { .. },
        ..
    } = act
    {
        return StepTiming {
            closes: data.closes(Verb::Vanish, "ability finished"),
            ..snap()
        };
    }
    // So is a card's name learned where it lies in no one's sight (a look at your own library,
    // which its prompt shows): only a permanent turns over (UI Conventions, "A card turning
    // face up").
    if let Act::Flip { object } = act
        && object.zone != ZoneName::Battlefield
    {
        return snap();
    }
    let own = data.timing(act.verb(), case);
    // A permanent that taps and then leaves in one beat (a Treasure, tapped and sacrificed for
    // mana: its payment is one event of the rules) has done turning as it begins to go, and
    // leaves from where it lay turned (UI Conventions, "streak": "Mana leaves as its source
    // begins to turn").
    if matches!(act, Act::Vanish { .. } | Act::Fly { .. })
        && let Some(mover) = mover_of(act)
    {
        let at = beside
            .iter()
            .position(|other| ptr::eq(*other, act))
            .unwrap_or(0);
        let turned = beside[..at]
            .iter()
            .any(|other| matches!(other, Act::Tap { .. }) && mover_of(other) == Some(mover));
        if turned {
            return StepTiming {
                after: vec![mover],
                ..own
            };
        }
    }
    // An arrow or a line is no turn of its beat: it goes with the step that calls for it and
    // holds up none after it. Each is cut in a frame as it ends, as Arena's is
    // (data/arena-motion.json, point: "It is cut, not faded, when it ends"), and what follows
    // waits for that. An arrow or a line of combat is drawn between things at rest: it waits
    // for the spell or ability it leaves to land. The line from an ability to its source does
    // not: it stretches as the ability lifts. The marks of one beat come up together, their
    // delay after its turn comes.
    if let Act::Point { span, .. } | Act::Mark { span, .. } = act {
        // What says "this, at that" is read before it goes: its end waits until it has stood
        // whole for the end's delay, and then nothing waits between its going and what it
        // pointed at moving (UI Conventions, Verbs, point). The line to a source says only
        // where an ability came from, which its lift has shown: its end waits its own delay
        // from the beat's start, the ability staying that moment after what it did, and the
        // two then go together.
        let led = matches!(act, Act::Point { .. });
        let to_source = matches!(
            act,
            Act::Point {
                arrow: Arrow::Source,
                ..
            }
        );
        if *span == Span::End {
            let read = led && !to_source;
            return if read {
                StepTiming {
                    delay: 0.0,
                    hold: Some(own.delay),
                    with: true,
                    leads: true,
                    ..own
                }
            } else {
                StepTiming { with: led, ..own }
            };
        }
        let after = match act {
            Act::Point { from, to, .. } if !to_source => [*from, *to]
                .into_iter()
                .filter(|end| !matches!(end, Subject::Player { .. }))
                .collect(),
            _ => Vec::new(),
        };
        return StepTiming {
            with: led,
            after,
            ..own
        };
    }
    // An announcement passes over the table while play goes on under it (UI Conventions,
    // Verbs, announce; Arena's cards move under its turn banner, `IEXcjBn3BXk 19:35.7`): its
    // beat does not wait for it. A coin is waited for: what follows it is what it decided. And
    // it has the table to itself: it is thrown only once the turn's banner has gone, so that
    // its flight is all that moves mid-table (Arena's outlasts its banner,
    // `WCknTZNbraE 17:22.13`).
    if let Act::Announce { notice } = act {
        let coin = matches!(notice, Notice::Coin { .. });
        return StepTiming {
            passes: !coin,
            alone: coin,
            ..own
        };
    }
    // A row opens for the permanent that appears in it before that is seen: it has made room
    // by the end of the delay (Arena's token is whole in its own slot from its first frame,
    // `UrGalw_f35w 17:38.63`). A hand dealt has no neighbours to move.
    if let Act::Appear { object, .. } = act
        && object.zone == ZoneName::Battlefield
    {
        return StepTiming { opens: true, ..own };
    }
    // The cards of one draw are all in the hand as the first leaves the library: the hand
    // opens their places once, and each card then flies to a place that stays where it is.
    // Compress multi-draw stagger so total stagger time never exceeds 1500 ms.
    if draws(act) {
        let count = beside.iter().filter(|other| draws(other)).count();
        let stagger = if count > 1 && own.stagger > 0.0 {
            #[allow(clippy::cast_precision_loss)]
            let capped = 1500.0 / count as f64;
            own.stagger.min(capped)
        } else {
            own.stagger
        };
        return StepTiming {
            ahead: true,
            stagger,
            ..own
        };
    }
    // The blow a creature deals back is one with the strike at it, the step `before`.
    if meets(act, before) {
        return StepTiming { with: true, ..own };
    }
    if let Act::Count {
        tally: Tally::Life { player, .. },
    } = act
    {
        // The life a streak of damage takes from a player goes as the streak lands on them.
        if let Some(
            streak @ Act::Streak {
                stream: Stream::Damage,
                to: Subject::Player { player: hit },
                ..
            },
        ) = before
            && hit == player
        {
            return StepTiming {
                delay: timing_of(streak, found, None, beside).duration,
                with: true,
                ..own
            };
        }
        // The life a strike takes from a player goes as its blow lands on them, not its own
        // delay after the strike's turn: the total flares on the frame the creature reaches
        // it.
        if let Some(
            strike @ Act::Strike {
                to: Subject::Player { player: hit },
                ..
            },
        ) = before
            && hit == player
        {
            let lands = data.strike().hit * timing_of(strike, found, None, beside).duration;
            return StepTiming {
                delay: lands,
                with: true,
                ..own
            };
        }
    }
    // A look at several cards of a library takes them off it one after another, each as long
    // as one alone (Arena's leave about 0.5 s apart: the data's stagger).
    if let Act::Lift {
        span: Span::Begin,
        handling: CardHandling::Library { count, .. },
    } = act
    {
        let count = f64::from(count.unwrap_or(0));
        if count > 1.0 && own.duration > 0.0 {
            return StepTiming {
                duration: own.duration + (count - 1.0) * own.stagger,
                each: Some(own.duration),
                ..own
            };
        }
    }
    // What follows a countered spell in its beat, the spell that countered it, waits until it
    // is gone, which is as its step ends (UI Conventions, "A countered spell": "It leaves
    // before the spell that countered it").
    if case == Some("countered") {
        return StepTiming {
            stagger: own.duration,
            ..own
        };
    }
    // One card of a batch leaving for exile begins a set time after the one before (UI
    // Conventions, "A batch exile": one at a time, about 0.3 s apart). Arena's instances are
    // of single cards, so the case's own values have no stagger for it.
    if case == Some("exiled")
        && let Some(stagger) = data.stagger(Verb::Vanish, "exiled")
    {
        return StepTiming { stagger, ..own };
    }
    own
}

/// The effects the step `act` begins, which finds the table `found`: its verb's that fit it
/// and do not leave its case out (`except`: a land played lands with no flash, as Arena's
/// does), and only its case's own where the case has any (a card that leaves for exile is lost
/// in light, and does not break up besides).
pub fn effects_of(act: &Act, found: &Table) -> Vec<StepEffect> {
    let data = Conventions::get();
    let case = case_of(act, found, &[]);
    let into = carried(act).map(|object| object.zone);
    let fit: Vec<_> = data
        .uses(act.verb())
        .iter()
        .filter(|used| {
            used.case.as_deref().is_none_or(|own| Some(own) == case)
                && used.into.is_none_or(|zone| Some(zone) == into)
                && !case.is_some_and(|case| used.except.iter().any(|left| left == case))
        })
        .collect();
    let own: Vec<_> = fit
        .iter()
        .copied()
        .filter(|used| used.case.is_some())
        .collect();
    // What a strike or a streak lands on: a card, an ability, or a player's life total. And
    // what a streak leaves, where that is something on the table and no player's pool.
    let struck = match act {
        Act::Strike { to, .. } | Act::Streak { to, .. } => Some(*to),
        _ => None,
    };
    // What a strike leaves is the creature that strikes.
    let source = match act {
        Act::Strike { source, .. } => Some(Subject::Object { slot: *source }),
        Act::Streak {
            from: Some(from), ..
        } if !matches!(from, Subject::Player { .. }) => Some(*from),
        _ => None,
    };
    let phases = data.strike();
    let chosen = if own.is_empty() { fit } else { own };
    chosen
        .into_iter()
        .filter_map(|used| {
            let look = data.look(&used.effect)?;
            let on = match used.on.as_deref() {
                None => None,
                Some("struck") => Some(struck?),
                Some("source") => Some(source?),
                Some(_) => return None,
            };
            Some(StepEffect {
                name: used.effect.clone(),
                when: if used.when == "end" {
                    Edge::End
                } else {
                    Edge::Start
                },
                on,
                into: match used.when.as_str() {
                    "blow" => Some(phases.hit),
                    "dash" => Some(phases.poise),
                    _ => None,
                },
                duration: look.duration,
                delay: used.delay.or(look.delay).unwrap_or(0.0),
                curve: curve(look.curve.as_deref()),
            })
        })
        .collect()
}

/// Each step's timing and effects, in playing order, as `script` plays over `before`: the
/// table of the view before, or with a reset, the table of the view the script came with.
/// Each step is timed with the table as it finds it, as [`replay`] plays the script. A step
/// whose act cannot be applied leaves the table as it was; the replay check, which a debug
/// build runs on every script it builds, fails such a script.
pub fn time_script(script: &Script, before: &Table) -> Vec<StepTiming> {
    let mut table = before.clone();
    if let Some(reset) = &script.reset {
        let _ = replay::number(&mut table, reset);
    }
    let mut timings = Vec::with_capacity(script.beats.iter().map(Vec::len).sum());
    for beat in &script.beats {
        let beside: Vec<&Act> = beat.iter().map(|step| &step.act).collect();
        for (i, step) in beat.iter().enumerate() {
            let before = i.checked_sub(1).map(|j| beside[j]);
            let mut timing = timing_of(&step.act, &table, before, &beside);
            timing.effects = effects_of(&step.act, &table);
            timings.push(timing);
            let _ = table.apply(&step.act);
        }
    }
    timings
}

/// Whether `act` is a card drawn, or a card flown as one is: the flight from a library to its
/// player's hand that the game calls a draw, or one no one at the table can read that an
/// effect puts there. A port of `draws` in `ui/src/stage/tracks.ts`.
fn draws(act: &Act) -> bool {
    matches!(act, Act::Fly { from: ZoneName::Library, why, object, .. }
        if object.zone == ZoneName::Hand && (*why == Why::Draw || object.name.is_none()))
}

/// Whether `act` is your draw of a card you see: the one flight in which a card is held up to
/// be read (UI Conventions, Verbs, fly). A port of `drawsToRead` in `tracks.ts`.
fn draws_to_read(act: &Act) -> bool {
    draws(act)
        && matches!(act, Act::Fly { object, .. }
            if object.zone_side.unwrap_or(object.owner) == Side::You && object.name.is_some())
}

/// Whether the strikes `a` and `b` are the two blows of one fight: each creature strikes the
/// other. A port of `meets` in `tracks.ts`.
fn meets(a: &Act, b: Option<&Act>) -> bool {
    matches!(
        (a, b),
        (
            Act::Strike { source: a_source, to: Subject::Object { slot: a_to }, .. },
            Some(Act::Strike { source: b_source, to: Subject::Object { slot: b_to }, .. }),
        ) if a_to == b_source && b_to == a_source
    )
}

/// The record the act carries as its own `object` field, where it has one. Unlike
/// [`Act::object`], a count's or a set's record is no act's own here, as on the web.
fn carried(act: &Act) -> Option<&ObjectView> {
    match act {
        Act::Fly { object, .. }
        | Act::Appear { object, .. }
        | Act::Flip { object }
        | Act::Tap { object }
        | Act::Showcase { object } => Some(object),
        Act::Vanish { object, .. } | Act::Strike { object, .. } | Act::Streak { object, .. } => {
            object.as_ref()
        }
        _ => None,
    }
}

/// What the step `act` moves: the creature that strikes, the ability that comes onto the
/// stack or leaves it, or the card its record is of. A port of `moverOf` and `subjectOf` in
/// `tracks.ts`.
#[must_use]
pub fn mover_of(act: &Act) -> Option<Subject> {
    match act {
        Act::Strike { source, .. } => Some(Subject::Object { slot: *source }),
        Act::Emerge { id, .. } => Some(Subject::Ability { id: *id }),
        Act::Vanish { subject, .. } if !matches!(subject, Subject::Player { .. }) => Some(*subject),
        _ => carried(act).map(|object| Subject::Object { slot: object.slot }),
    }
}
