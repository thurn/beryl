//! The design language's timing data, `data/ui-conventions.json`, read once: each verb's
//! duration, delay, stagger and curve, its cases' own, the effects its steps begin, and the
//! few numbers the timing rules take from the data beside them: a strike's phases, how long a
//! hand takes to regroup and how far a tap turns before its row makes room.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::{Curve, Room, StepTiming};
use crate::protocol::ZoneName;
use crate::script::Verb;

/// The data file, embedded so that the session times scripts with no file to find.
pub(super) const DATA: &str = include_str!("../../../../../../data/ui-conventions.json");

/// The parts of the data the timing rules read.
pub(crate) struct Conventions {
    verbs: HashMap<String, VerbTimings>,
    effects: HashMap<String, Look>,
}

/// One verb's timing: its own, each case's, and the effects its steps begin.
struct VerbTimings {
    whole: StepTiming,
    cases: HashMap<String, StepTiming>,
    uses: Vec<Use>,
    phases: Option<Phases>,
    closes: HashMap<String, f64>,
    stagger: HashMap<String, f64>,
    regroup: Option<f64>,
    room: Option<f64>,
}

/// How an effect is played where a step begins it.
#[derive(Debug, Deserialize)]
pub(crate) struct Look {
    pub duration: Option<f64>,
    pub delay: Option<f64>,
    pub curve: Option<String>,
}

/// One of the effects a verb's steps begin, as the data says it: at which end of the step or
/// as its blow lands (`blow`) or its dash begins (`dash`), of which steps (those of a case, or
/// those that leave their card in a zone, but not those of the cases in `except`), and on
/// what, where that is not what the step is about (`struck` or `source`).
#[derive(Debug, Deserialize)]
pub(crate) struct Use {
    pub effect: String,
    pub when: String,
    pub case: Option<String>,
    pub into: Option<ZoneName>,
    #[serde(default)]
    pub except: Vec<String>,
    pub on: Option<String>,
    /// How long after its edge of the step it begins, where that is not its look's own delay.
    pub delay: Option<f64>,
}

/// Where in a strike its creature dashes from its poise and where its blow lands, as shares
/// of the strike's time.
#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct Phases {
    pub poise: f64,
    pub hit: f64,
}

#[derive(Deserialize)]
struct Data {
    verbs: Vec<VerbData>,
    effects: Vec<EffectData>,
}

#[derive(Deserialize)]
struct VerbData {
    name: String,
    values: Values,
    #[serde(default)]
    cases: HashMap<String, Option<Values>>,
    #[serde(default)]
    effects: Vec<Use>,
    phases: Option<Phases>,
    #[serde(default)]
    closes: HashMap<String, f64>,
    #[serde(default)]
    stagger: HashMap<String, f64>,
    regroup: Option<f64>,
    room: Option<f64>,
}

#[derive(Clone, Deserialize)]
struct Values {
    duration: Option<f64>,
    delay: Option<f64>,
    stagger: Option<f64>,
    curve: Option<String>,
}

#[derive(Deserialize)]
struct EffectData {
    name: String,
    values: Look,
}

/// No motion: the step is over in the tick that begins it (`SNAP` in
/// `ui/src/stage/tracks.ts`).
pub(crate) fn snap() -> StepTiming {
    StepTiming {
        case: None,
        duration: 0.0,
        delay: 0.0,
        stagger: 0.0,
        curve: Curve::Linear,
        each: None,
        with: false,
        after: Vec::new(),
        hold: None,
        leads: false,
        passes: false,
        alone: false,
        opens: false,
        ahead: false,
        closes: None,
        room: Room::Step,
        phases: Vec::new(),
        effects: Vec::new(),
    }
}

/// The curve the data names: CSS's of the same names, and `none`, a verb in which nothing
/// travels, changes evenly. A name not known is linear (`CURVES` in `timing.ts`).
pub(crate) fn curve(name: Option<&str>) -> Curve {
    match name {
        Some("ease-in") => Curve::EaseIn,
        Some("ease-out") => Curve::EaseOut,
        Some("ease-in-out") => Curve::EaseInOut,
        _ => Curve::Linear,
    }
}

/// The timing `values` give, each number or curve not given being `over`'s (`timing` in
/// `timing.ts`).
fn timing(values: &Values, over: Option<&StepTiming>) -> StepTiming {
    let or = |own: Option<f64>, of: fn(&StepTiming) -> f64| own.or(over.map(of)).unwrap_or(0.0);
    StepTiming {
        duration: or(values.duration, |t| t.duration),
        delay: or(values.delay, |t| t.delay),
        stagger: or(values.stagger, |t| t.stagger),
        curve: match values.curve.as_deref() {
            Some(name) if !name.is_empty() => curve(Some(name)),
            _ => over.map_or(Curve::Linear, |t| t.curve),
        },
        ..snap()
    }
}

impl Conventions {
    /// The data, parsed on first use.
    pub(crate) fn get() -> &'static Conventions {
        static CONVENTIONS: OnceLock<Conventions> = OnceLock::new();
        CONVENTIONS.get_or_init(|| Conventions::parse(DATA))
    }

    fn parse(text: &str) -> Conventions {
        let data: Data = serde_json::from_str(text).expect("data/ui-conventions.json parses");
        let verbs = data
            .verbs
            .into_iter()
            .map(|verb| {
                let whole = timing(&verb.values, None);
                let cases = verb
                    .cases
                    .iter()
                    .filter_map(|(name, values)| {
                        Some((name.clone(), timing(values.as_ref()?, Some(&whole))))
                    })
                    .collect();
                let timings = VerbTimings {
                    whole,
                    cases,
                    uses: verb.effects,
                    phases: verb.phases,
                    closes: verb.closes,
                    stagger: verb.stagger,
                    regroup: verb.regroup,
                    room: verb.room,
                };
                (verb.name, timings)
            })
            .collect();
        let effects = data
            .effects
            .into_iter()
            .map(|effect| (effect.name, effect.values))
            .collect();
        Conventions { verbs, effects }
    }

    /// The timing of a step of `verb` and of `case`: the case's own, or the verb's where the
    /// case is none or has none, or none at all for a verb the data does not list.
    pub(crate) fn timing(&self, verb: Verb, case: Option<&str>) -> StepTiming {
        let Some(timings) = self.verbs.get(verb.name()) else {
            return snap();
        };
        case.and_then(|case| timings.cases.get(case))
            .unwrap_or(&timings.whole)
            .clone()
    }

    /// The effects `verb`'s steps begin.
    pub(crate) fn uses(&self, verb: Verb) -> &[Use] {
        self.verbs.get(verb.name()).map_or(&[], |t| &t.uses)
    }

    /// How the effect named `name` is played.
    pub(crate) fn look(&self, name: &str) -> Option<&Look> {
        self.effects.get(name)
    }

    /// Where a strike's creature dashes and where its blow lands (the `strike` verb's
    /// `phases`, `STRIKE` in `ui/src/stage/tracks.ts`).
    pub(crate) fn strike(&self) -> Phases {
        self.verbs
            .get(Verb::Strike.name())
            .and_then(|t| t.phases)
            .expect("the strike verb has phases in data/ui-conventions.json")
    }

    /// How long the cards that stay in a hand take to go to their new places as a card joins
    /// or leaves it (the `fly` verb's `regroup`).
    pub(crate) fn regroup(&self) -> f64 {
        self.verbs
            .get(Verb::Fly.name())
            .and_then(|t| t.regroup)
            .expect("the fly verb has a regroup in data/ui-conventions.json")
    }

    /// How far through a tap or an untap its permanent has turned, and its row begins to make
    /// room (the `tap` verb's `room`).
    pub(crate) fn turned(&self) -> f64 {
        self.verbs
            .get(Verb::Tap.name())
            .and_then(|t| t.room)
            .expect("the tap verb has a room in data/ui-conventions.json")
    }

    /// How long what a step of `verb` and `case` left room for takes to close up after it
    /// (the verb's `closes`), where the data says.
    pub(crate) fn closes(&self, verb: Verb, case: &str) -> Option<f64> {
        self.verbs.get(verb.name())?.closes.get(case).copied()
    }

    /// How long the next step waits after a step of `verb` and `case` begins, where a rule
    /// sets that apart from the case's measured values (the verb's `stagger`).
    pub(crate) fn stagger(&self, verb: Verb, case: &str) -> Option<f64> {
        self.verbs.get(verb.name())?.stagger.get(case).copied()
    }
}
