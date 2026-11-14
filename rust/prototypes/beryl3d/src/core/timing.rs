//! Verb timings from data/ui-conventions.json, the entries the web interface reads. The caller
//! reads the file; this only interprets the parsed value.

use serde_json::Value;

use super::motion::{Curve, Timing};

const MS: u64 = 1_000_000;

#[derive(Clone, Copy)]
pub struct Timings {
    pub fly: Timing,
    pub land_played: Timing,
    pub opponent_land_played: Timing,
    pub own_spell_cast: Timing,
    pub opponent_spell_cast: Timing,
    pub resolves: Timing,
    pub discard: Timing,
    pub appear: Timing,
    pub vanish: Timing,
    pub tap: Timing,
    pub untap: Timing,
    pub flip: Timing,
    /// Cards that stay in a hand as one joins or leaves it slide to their new places in about a
    /// sixth of a second (the `fly` verb's description).
    pub slide: Timing,
    /// A hand card lifting under the pointer.
    pub hover: Timing,
    /// How long each verb that the prototype shows as an instant change holds its beat, indexed
    /// by [`Other`].
    pub others: [Timing; 10],
}

/// The verbs the prototype draws as an instant change. Their beats still last as long as the
/// data says, so the game keeps its pace.
#[derive(Clone, Copy)]
pub enum Other {
    Emerge,
    Strike,
    Streak,
    Count,
    Set,
    Showcase,
    Dwell,
    Announce,
    Point,
    Mark,
}

impl Timings {
    pub fn from_json(conventions: &Value) -> Timings {
        let verb = |name: &str, case: Option<&str>| timing(conventions, name, case);
        Timings {
            fly: verb("fly", None),
            land_played: verb("fly", Some("land played")),
            opponent_land_played: verb("fly", Some("opponent land played")),
            own_spell_cast: verb("fly", Some("own spell cast")),
            opponent_spell_cast: verb("fly", Some("opponent spell cast")),
            resolves: verb("fly", Some("permanent resolves")),
            discard: verb("fly", Some("discard")),
            appear: verb("appear", None),
            vanish: verb("vanish", None),
            tap: verb("tap", Some("tap")),
            untap: verb("tap", Some("untap")),
            flip: verb("flip", None),
            slide: Timing {
                delay: 0,
                duration: 165 * MS,
                stagger: 0,
                curve: Curve::EASE_OUT,
            },
            hover: Timing {
                delay: 0,
                duration: 120 * MS,
                stagger: 0,
                curve: Curve::EASE_OUT,
            },
            others: [
                verb("emerge", None),
                verb("strike", None),
                verb("streak", None),
                verb("count", None),
                verb("set", None),
                verb("showcase", None),
                verb("dwell", None),
                verb("announce", None),
                verb("point", None),
                verb("mark", None),
            ],
        }
    }

    /// Every timing played `speed` times as fast.
    pub fn scaled(&self, speed: u64) -> Timings {
        let scale = |t: Timing| Timing {
            delay: t.delay / speed,
            duration: t.duration / speed,
            stagger: t.stagger / speed,
            curve: t.curve,
        };
        Timings {
            fly: scale(self.fly),
            land_played: scale(self.land_played),
            opponent_land_played: scale(self.opponent_land_played),
            own_spell_cast: scale(self.own_spell_cast),
            opponent_spell_cast: scale(self.opponent_spell_cast),
            resolves: scale(self.resolves),
            discard: scale(self.discard),
            appear: scale(self.appear),
            vanish: scale(self.vanish),
            tap: scale(self.tap),
            untap: scale(self.untap),
            flip: scale(self.flip),
            slide: scale(self.slide),
            hover: self.hover,
            others: self.others.map(scale),
        }
    }

    pub fn other(&self, other: Other) -> Timing {
        self.others[other as usize]
    }
}

/// A verb's values, with the named case's non-null values over them.
fn timing(conventions: &Value, name: &str, case: Option<&str>) -> Timing {
    let entry = conventions["verbs"]
        .as_array()
        .and_then(|verbs| verbs.iter().find(|v| v["name"] == name))
        .unwrap_or_else(|| panic!("ui-conventions.json has no verb {name}"));
    let mut result = Timing {
        delay: 0,
        duration: 0,
        stagger: 0,
        curve: Curve::LINEAR,
    };
    apply(&mut result, &entry["values"]);
    if let Some(case) = case {
        apply(&mut result, &entry["cases"][case]);
    }
    result
}

fn apply(timing: &mut Timing, values: &Value) {
    let ms = |key: &str| values[key].as_f64().map(|v| (v * 1e6) as u64);
    if let Some(v) = ms("duration") {
        timing.duration = v;
    }
    if let Some(v) = ms("delay") {
        timing.delay = v;
    }
    if let Some(v) = ms("stagger") {
        timing.stagger = v;
    }
    match values["curve"].as_str() {
        Some("ease-out") => timing.curve = Curve::EASE_OUT,
        Some("ease-in") => timing.curve = Curve::EASE_IN,
        Some(_) => timing.curve = Curve::LINEAR,
        None => {}
    }
}
