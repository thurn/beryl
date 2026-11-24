use crate::Refusal;

/// Random draws for one host or a replay of one entry's recorded outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chance {
    source: Source,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Fresh(u64),
    Replaying {
        recorded: Vec<u32>,
        next: usize,
        fresh: Option<u64>,
    },
}

impl Chance {
    pub fn fresh(seed: u64) -> Self {
        Self {
            source: Source::Fresh(seed),
        }
    }

    pub fn replaying(recorded: Vec<u32>) -> Self {
        Self {
            source: Source::Replaying {
                recorded,
                next: 0,
                fresh: None,
            },
        }
    }

    pub(crate) fn replay_and_advance(&self, recorded: Vec<u32>) -> Self {
        let Source::Fresh(state) = self.source else {
            unreachable!("only the host's fresh stream is resumed");
        };
        Self {
            source: Source::Replaying {
                recorded,
                next: 0,
                fresh: Some(state),
            },
        }
    }

    pub(crate) fn update_fresh(&self, chance: &mut Self) -> bool {
        let Source::Replaying {
            ref recorded,
            next,
            fresh: Some(state),
        } = self.source
        else {
            unreachable!("replay did not track the fresh stream");
        };
        if next != recorded.len() {
            return false;
        }
        chance.source = Source::Fresh(state);
        true
    }

    /// Draws a zero-based outcome below `upper`. The caller records it in
    /// a change so replay consumes the same outcomes in the same order.
    pub fn below(&mut self, upper: u32) -> Result<u32, Refusal> {
        if upper == 0 {
            return Err(Refusal::INVALID);
        }
        match &mut self.source {
            Source::Fresh(state) => Ok(draw_below(state, upper)),
            Source::Replaying {
                recorded,
                next,
                fresh,
            } => {
                let outcome = recorded
                    .get(*next)
                    .copied()
                    .filter(|&outcome| outcome < upper)
                    .ok_or(Refusal::INVALID)?;
                if let Some(state) = fresh {
                    draw_below(state, upper);
                }
                *next += 1;
                Ok(outcome)
            }
        }
    }
}

fn draw_below(state: &mut u64, upper: u32) -> u32 {
    let upper = u64::from(upper);
    // Reject the incomplete interval so each remainder has equal probability.
    let threshold = upper.wrapping_neg() % upper;
    loop {
        let draw = splitmix64(state);
        if draw >= threshold {
            return (draw % upper) as u32;
        }
    }
}

/// `SplitMix64` advances a full-period state and mixes it into each draw.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut draw = *state;
    draw = (draw ^ (draw >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    draw = (draw ^ (draw >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    draw ^ (draw >> 31)
}
