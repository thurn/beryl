use crate::Refusal;

/// Random draws for one host or a replay of one entry's recorded outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chance {
    source: Source,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Fresh(u64),
    Replaying { recorded: Vec<u32>, next: usize },
}

impl Chance {
    pub fn fresh(seed: u64) -> Self {
        Self {
            source: Source::Fresh(seed),
        }
    }

    pub fn replaying(recorded: Vec<u32>) -> Self {
        Self {
            source: Source::Replaying { recorded, next: 0 },
        }
    }

    /// Draws a zero-based outcome below `upper`. The caller records it in
    /// `Change::Random` so replay consumes the same outcomes in the same order.
    pub fn below(&mut self, upper: u32) -> Result<u32, Refusal> {
        if upper == 0 {
            return Err(Refusal::Invalid);
        }
        match &mut self.source {
            Source::Fresh(state) => {
                let upper = u64::from(upper);
                // Reject the incomplete interval so each remainder has the same probability.
                let threshold = upper.wrapping_neg() % upper;
                loop {
                    let draw = splitmix64(state);
                    if draw >= threshold {
                        return Ok((draw % upper) as u32);
                    }
                }
            }
            Source::Replaying { recorded, next } => {
                let outcome = recorded
                    .get(*next)
                    .copied()
                    .filter(|&outcome| outcome < upper)
                    .ok_or(Refusal::Invalid)?;
                *next += 1;
                Ok(outcome)
            }
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
