//! A beat laid out in time (docs/beryl-3d.md, "Timing a script"): when each of a beat's steps
//! begins and ends, from the timings the session sent with its script. The session chooses
//! each step's case and waits; a client still lays the beat out, because two of the inputs
//! are known only to it: since when each span on screen has stood whole, and until when
//! something passes over the table.

use beryl_protocol::script::{Act, Step, StepTiming, Subject, mover_of};

/// When one step of a beat plays, in stage milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Laid {
    /// When its turn in its beat came, which its delay counts from.
    pub turn: f64,
    /// When it begins.
    pub from: f64,
    /// When it ends.
    pub to: f64,
    /// Its act is applied before it begins, `at` the time its delay does, and what makes room
    /// for it has done so `by` the time it begins, or by the time the first of the steps the
    /// table is then open for does (`StepTiming::opens` and `StepTiming::ahead`).
    pub opens: Option<Opens>,
}

/// When the act of a step is applied ahead of it, and when the room it needs is open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opens {
    pub at: f64,
    pub by: f64,
}

/// The steps of `beat`, timed by `timings` (one for each step, in order), as they play from
/// stage time `at`, written into `steps`; and when the beat is over, which is when its last
/// motion is. A port of `laid` in `ui/src/stage/beat.ts`, whose comment follows; the
/// session has already timed each step by the table it finds, so the port reads the timings
/// instead of working them out.
///
/// A step begins its delay after its turn comes, and never before the step before it; the
/// next step's turn is a stagger after. One played `with` the step before it takes that
/// step's turn, and one that is drawn between things at rest (`after`) waits for the steps
/// of the beat that move them. One that ends a span waits until the span has stood whole
/// for its hold (`hold`), `whole` saying since when the span a step ends has; and what
/// follows one that leads (`leads`) waits until it is over. The beat is over with the last
/// step that does not pass over the table (`passes`): one that does holds the beat only
/// until it has begun, and plays on over what follows; and a step that has the table to
/// itself (`alone`) waits until `passed`, the stage time by which everything that passes
/// over the table now is over. The act of one that the table opens for (`opens`) is applied
/// as its delay begins, and the act of one that goes `ahead` as the act of the step before
/// it is, if that goes ahead too: the first of them begins as its act is applied, and the
/// hand makes room for every card of a draw at once.
///
/// # Panics
///
/// If `timings` holds fewer entries than `beat`.
pub fn beat(
    beat: &[Step],
    timings: &[StepTiming],
    at: f64,
    whole: impl Fn(&Act) -> Option<f64>,
    passed: f64,
    steps: &mut Vec<Laid>,
) -> f64 {
    steps.clear();
    steps.resize(beat.len(), Laid::default());
    lay(beat, timings, at, whole, (passed, None), steps)
}

/// `beat`, writing the steps into `steps`, which holds one entry for each step, so that laying
/// a beat out again allocates nothing. `lead` is the timing of a step the client puts first in
/// the beat, before the session's: a closed prompt's tray letting its cards go, which the web
/// interface puts first in the beat of the answering script it plays in (`lowered` in
/// `ui/src/stage/prompt.ts`). It moves nothing a later step waits for and ends no span, and is
/// laid out like any other step, so the beat lasts until it is over.
///
/// # Panics
///
/// If `timings` or `steps` holds fewer entries than `beat`.
pub fn lay(
    beat: &[Step],
    timings: &[StepTiming],
    at: f64,
    whole: impl Fn(&Act) -> Option<f64>,
    (passed, lead): (f64, Option<&StepTiming>),
    steps: &mut [Laid],
) -> f64 {
    let mut cursor = Cursor::new(at);
    if let Some(timing) = lead {
        cursor.place(timing, at, (f64::NEG_INFINITY, None), passed);
    }
    for (i, step) in beat.iter().enumerate() {
        let timing = &timings[i];
        // When each thing a step of the beat moves has come to rest: the latest end of the
        // steps before this one that move it.
        let landed = |name: &Subject| {
            beat[..i]
                .iter()
                .zip(steps[..i].iter())
                .filter(|(earlier, _)| mover_of(&earlier.act).as_ref() == Some(name))
                .map(|(_, laid)| laid.to)
                .reduce(f64::max)
        };
        let after = (timing.after.iter())
            .map(|name| landed(name).unwrap_or(at))
            .fold(f64::NEG_INFINITY, f64::max);
        // When the span this step ends will have stood whole for its hold.
        let held = timing
            .hold
            .and_then(|hold| whole(&step.act).map(|since| since + hold));
        steps[i] = cursor.place(timing, at, (after, held), passed);
    }
    cursor.end
}

/// Where the laying out of a beat has got to (`lay`): when the last step began (`from`), when
/// the next step's turn comes, the turn the last step took, when the beat is over so far, when
/// the last act was applied, when the first of the steps whose acts went ahead and that have
/// yet to begin does (`open`), and whether the step before went ahead (`led`).
struct Cursor {
    from: f64,
    turn: f64,
    last: f64,
    end: f64,
    applied: f64,
    open: f64,
    led: bool,
}

impl Cursor {
    fn new(at: f64) -> Self {
        Self {
            from: at,
            turn: at,
            last: at,
            end: at,
            applied: at,
            open: at,
            led: false,
        }
    }

    /// Lays out the next step of a beat that starts at `at`, timed by `timing`: `after` is when
    /// the things it waits for have come to rest, `held` when the span it ends will have stood
    /// whole for its hold, and `passed` when everything passing over the table is over.
    fn place(
        &mut self,
        timing: &StepTiming,
        at: f64,
        (after, held): (f64, Option<f64>),
        passed: f64,
    ) -> Laid {
        let own = if timing.with { self.last } else { self.turn };
        let rested = [
            if timing.with { self.from } else { own },
            after,
            held.unwrap_or(at),
            if timing.alone { passed } else { at },
        ]
        .into_iter()
        .fold(f64::NEG_INFINITY, f64::max);
        self.from = self.from.max(rested + timing.delay);
        // Its act goes ahead of it no sooner than the act of the step before it.
        let opens = if timing.opens {
            self.applied.max(rested)
        } else if timing.ahead && self.led {
            self.applied
        } else {
            self.from
        };
        self.applied = opens;
        self.led = timing.ahead;
        if self.open <= opens {
            self.open = self.from;
        }
        if !timing.with {
            self.turn = own + timing.stagger;
        }
        self.last = own;
        let began = self.from;
        let to = began + timing.duration;
        self.end = self.end.max(if timing.passes { began } else { to });
        if timing.leads {
            self.from = to;
        }
        Laid {
            turn: own,
            from: began,
            to,
            opens: (opens < began).then_some(Opens {
                at: opens,
                by: self.open,
            }),
        }
    }
}
