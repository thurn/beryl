//! The integration branch's lease while a landing holds it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use ignite_core::protocol::tickets::{LeaseOp, LeaseReply};
use ignite_core::traits::{Hub, Result};

/// How long the hub keeps the integration branch's lease without a renewal.
pub(super) const LEASE_TTL_S: u64 = 180;
/// How often a held lease is renewed.
const RENEW_EVERY: Duration = Duration::from_secs(60);

/// The integration branch's lease while this landing holds it. A thread renews it every
/// [`RENEW_EVERY`]; dropping the guard stops the thread and releases the lease.
///
/// A renewal the hub answers as busy means another landing holds the branch now (this one's
/// lease expired, for example while the machine slept). The guard remembers that, and
/// [`HeldLease::still_held`] tells the landing before it pushes without the lease.
pub(super) struct HeldLease {
    hub: Arc<dyn Hub>,
    name: String,
    holder: String,
    lost: Arc<AtomicBool>,
    stop: Arc<(Mutex<bool>, Condvar)>,
    renewer: Option<JoinHandle<()>>,
}

/// Renews the lease once.
fn renew_once(hub: &dyn Hub, name: &str, holder: &str, lost: &AtomicBool) -> bool {
    let op = LeaseOp::Renew {
        holder: holder.to_owned(),
        ttl_s: LEASE_TTL_S,
        payload: None,
        if_revision: None,
    };
    note_renewal(hub.lease(name, op), name, lost)
}

/// Reads the hub's answer to a renewal. Returns false, and records the loss, when the hub says
/// another holder has the lease. A hub that cannot be reached proves nothing, so that is
/// reported and the lease is kept.
fn note_renewal(reply: Result<LeaseReply>, name: &str, lost: &AtomicBool) -> bool {
    match reply {
        Ok(LeaseReply::Busy(_)) => {
            eprintln!("ignite: the {name} lease was lost to another holder");
            lost.store(true, Ordering::SeqCst);
            false
        }
        Ok(LeaseReply::Held(_)) => true,
        Err(error) => {
            eprintln!("ignite: the {name} lease was not renewed: {error}");
            !lost.load(Ordering::SeqCst)
        }
    }
}

impl HeldLease {
    pub(super) fn start(hub: Arc<dyn Hub>, name: String, holder: String) -> HeldLease {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let lost = Arc::new(AtomicBool::new(false));
        let renewer = {
            let (hub, name, holder, stop, lost) = (
                hub.clone(),
                name.clone(),
                holder.clone(),
                stop.clone(),
                lost.clone(),
            );
            std::thread::spawn(move || {
                let (stopped, wake) = &*stop;
                let mut guard = stopped.lock().unwrap_or_else(PoisonError::into_inner);
                loop {
                    let (next, _) = wake
                        .wait_timeout_while(guard, RENEW_EVERY, |stopped| !*stopped)
                        .unwrap_or_else(PoisonError::into_inner);
                    guard = next;
                    if *guard {
                        return;
                    }
                    if !renew_once(hub.as_ref(), &name, &holder, &lost) {
                        return;
                    }
                }
            })
        };
        HeldLease {
            hub,
            name,
            holder,
            lost,
            stop,
            renewer: Some(renewer),
        }
    }

    /// Whether the lease is still this landing's: asks the hub to renew it now, which also
    /// restarts its time to live for the push that follows.
    pub(super) fn still_held(&self) -> bool {
        renew_once(self.hub.as_ref(), &self.name, &self.holder, &self.lost)
            && !self.lost.load(Ordering::SeqCst)
    }
}

impl Drop for HeldLease {
    fn drop(&mut self) {
        let (stopped, wake) = &*self.stop;
        *stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
        if let Some(renewer) = self.renewer.take() {
            let _ = renewer.join();
        }
        // A lease that was lost belongs to someone else now; releasing it would free theirs.
        if self.lost.load(Ordering::SeqCst) {
            return;
        }
        let op = LeaseOp::Release {
            holder: self.holder.clone(),
            if_revision: None,
        };
        if let Err(error) = self.hub.lease(&self.name, op) {
            eprintln!("ignite: the {} lease was not released: {error}", self.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use ignite_core::protocol::tickets::{Lease, LeaseBusy};
    use ignite_core::time::Timestamp;
    use ignite_core::traits::Error;

    use super::*;

    fn held() -> LeaseReply {
        LeaseReply::Held(Lease {
            name: "staging".to_owned(),
            holder: "m:T.1".to_owned(),
            machine: String::new(),
            expires_at: Timestamp::EPOCH,
            payload: serde_json::Value::Null,
            waiters: Vec::new(),
        })
    }

    #[test]
    fn a_busy_renewal_marks_the_lease_lost_and_an_outage_does_not() {
        let lost = AtomicBool::new(false);
        assert!(note_renewal(Ok(held()), "staging", &lost));
        assert!(note_renewal(
            Err(Error::Unreachable("down".to_owned())),
            "staging",
            &lost
        ));
        assert!(!lost.load(Ordering::SeqCst));
        let busy = Ok(LeaseReply::Busy(LeaseBusy {
            holder: Some("m:T.2".to_owned()),
            expires_at: None,
            position: None,
            seq: None,
        }));
        assert!(!note_renewal(busy, "staging", &lost));
        assert!(lost.load(Ordering::SeqCst));
    }
}
