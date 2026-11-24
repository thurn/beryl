use std::sync::Mutex;

use crate::time::Timestamp;
use crate::traits::Clock;

use super::lock;

/// A clock that moves only when the test moves it.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Timestamp>,
}

impl FakeClock {
    pub fn new(now: Timestamp) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    pub fn set(&self, now: Timestamp) {
        *lock(&self.now) = now;
    }

    pub fn advance(&self, seconds: f64) {
        let mut now = lock(&self.now);
        *now = now.plus_seconds(seconds);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *lock(&self.now)
    }
}
