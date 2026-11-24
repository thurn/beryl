use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::SessionId;

/// The engine and manual table share one sequence when they emit to the same stage.
#[derive(Clone, Default)]
pub struct SessionIds(Arc<AtomicU32>);

impl SessionIds {
    pub fn allocate(&self) -> SessionId {
        let previous = self
            .0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                last.checked_add(1)
            })
            .expect("session numbers exhausted");
        SessionId(previous + 1)
    }
}
