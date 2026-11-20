//! [`FakeCliPool`], a [`CliPool`] held in memory for the dispatcher's tests.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use ignite_core::cli::{Hold, Slot};
use ignite_core::ids::SessionId;
use ignite_core::protocol::hub::Reservation;
use ignite_core::traits::{Error, Result};

use super::{CliPool, Resume, SessionCheck, lock};

/// A message [`FakeCliPool`] was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SentMessage {
    pub session: SessionId,
    pub text: String,
    pub urgent: bool,
}

/// A justiciar session [`FakeCliPool`] was asked to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JusticiarStart {
    pub app: String,
    pub prompt: String,
    pub model: String,
    pub effort: String,
}

/// A [`CliPool`] held in memory. `begin` names each new session `cli-<app>-<n>` with `n`
/// counting from 1 in hexadecimal, padded to 8 digits, and answers `check` with `Idle` for it;
/// a test sets other answers, slots and holds directly.
#[derive(Debug, Default)]
pub struct FakeCliPool {
    begun: Mutex<Vec<(Reservation, String, Option<String>)>>,
    /// The `resume` of each `begin`, in the same order as `begun`.
    resumes: Mutex<Vec<Option<Resume>>>,
    justiciars: Mutex<Vec<JusticiarStart>>,
    /// `begin` fails with this error while it is set.
    refuse: Mutex<Option<String>>,
    sent: Mutex<Vec<SentMessage>>,
    checks: Mutex<BTreeMap<SessionId, SessionCheck>>,
    slots: Mutex<BTreeMap<String, Vec<Slot>>>,
    holds: Mutex<BTreeMap<String, Hold>>,
    /// Apps no probe has passed for yet, whose slots are not offered.
    unprobed: Mutex<BTreeSet<String>>,
}

impl FakeCliPool {
    /// Every `begin` so far: the reservation, model and effort.
    pub fn begun(&self) -> Vec<(Reservation, String, Option<String>)> {
        lock(&self.begun).clone()
    }

    /// The `resume` argument of every `begin` so far, in order.
    pub fn resumes(&self) -> Vec<Option<Resume>> {
        lock(&self.resumes).clone()
    }

    /// Every justiciar session started so far, in order.
    pub fn justiciars(&self) -> Vec<JusticiarStart> {
        lock(&self.justiciars).clone()
    }

    /// Makes every later `begin` fail with `error`, or with `None` succeed again.
    pub fn refuse(&self, error: Option<&str>) {
        *lock(&self.refuse) = error.map(str::to_owned);
    }

    /// Every message so far, in order.
    pub fn sent(&self) -> Vec<SentMessage> {
        lock(&self.sent).clone()
    }

    pub fn set_check(&self, session: &SessionId, answer: SessionCheck) {
        lock(&self.checks).insert(session.clone(), answer);
    }

    pub fn set_slots(&self, app: &str, slots: Vec<Slot>) {
        lock(&self.slots).insert(app.to_owned(), slots);
    }

    /// Records whether a probe turn has passed for `app`; until one has, its slots are not
    /// offered. Every app counts as probed until this says otherwise.
    pub fn set_probed(&self, app: &str, probed: bool) {
        let mut unprobed = lock(&self.unprobed);
        if probed {
            unprobed.remove(app);
        } else {
            unprobed.insert(app.to_owned());
        }
    }

    /// Puts `hold.app` on `hold`, or with `None` ends `app`'s hold.
    pub fn set_hold(&self, app: &str, hold: Option<Hold>) {
        let mut holds = lock(&self.holds);
        match hold {
            Some(hold) => holds.insert(app.to_owned(), hold),
            None => holds.remove(app),
        };
    }
}

impl CliPool for FakeCliPool {
    fn begin(
        &self,
        reservation: &Reservation,
        model: &str,
        effort: Option<&str>,
        resume: Option<&Resume>,
    ) -> Result<SessionId> {
        let app = reservation
            .session
            .as_str()
            .strip_prefix("cli-")
            .and_then(|rest| rest.split_once("-slot-"))
            .map(|(app, _)| app.to_owned())
            .ok_or_else(|| Error::Invalid(format!("{} is not a CLI slot", reservation.session)))?;
        if let Some(error) = lock(&self.refuse).clone() {
            return Err(Error::Io(error));
        }
        lock(&self.resumes).push(resume.cloned());
        let mut begun = lock(&self.begun);
        begun.push((
            reservation.clone(),
            model.to_owned(),
            effort.map(str::to_owned),
        ));
        let session = SessionId::new(format!("cli-{app}-{:08x}", begun.len()))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        lock(&self.checks).insert(session.clone(), SessionCheck::Idle);
        Ok(session)
    }

    fn message(&self, session: &SessionId, text: &str, urgent: bool) -> Result<()> {
        lock(&self.sent).push(SentMessage {
            session: session.clone(),
            text: text.to_owned(),
            urgent,
        });
        Ok(())
    }

    fn check(&self, session: &SessionId) -> SessionCheck {
        lock(&self.checks)
            .get(session)
            .copied()
            .unwrap_or(SessionCheck::Gone)
    }

    fn slots(&self, app: &str) -> Vec<Slot> {
        lock(&self.slots).get(app).cloned().unwrap_or_default()
    }

    fn held(&self, app: &str) -> Option<Hold> {
        lock(&self.holds).get(app).cloned()
    }

    fn offers(&self, app: &str) -> bool {
        !lock(&self.unprobed).contains(app) && self.held(app).is_none()
    }

    fn begin_justiciar(
        &self,
        app: &str,
        prompt: &str,
        model: &str,
        effort: &str,
    ) -> Result<SessionId> {
        let mut started = lock(&self.justiciars);
        started.push(JusticiarStart {
            app: app.to_owned(),
            prompt: prompt.to_owned(),
            model: model.to_owned(),
            effort: effort.to_owned(),
        });
        let session = SessionId::new(format!("cli-{app}-j{:07x}", started.len()))
            .map_err(|e| Error::Invalid(e.to_string()))?;
        lock(&self.checks).insert(session.clone(), SessionCheck::Idle);
        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::ids::{MachineId, TicketId};

    #[test]
    fn the_fake_begins_sessions_and_answers_checks() {
        let pool = FakeCliPool::default();
        let reservation = Reservation {
            ticket: TicketId::new("T.1").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            session: SessionId::new("cli-claude-slot-1").unwrap(),
            expires_at: "2026-11-20T12:00:00.000Z".parse().unwrap(),
            code: "c0de".to_owned(),
        };
        let session = pool
            .begin(&reservation, "opus", Some("high"), None)
            .unwrap();
        assert_eq!(session.as_str(), "cli-claude-00000001");
        assert_eq!(pool.check(&session), SessionCheck::Idle);
        pool.set_check(&session, SessionCheck::Running);
        assert_eq!(pool.check(&session).as_str(), "running");
        let stranger = SessionId::new("local_c8470108").unwrap();
        assert_eq!(pool.check(&stranger), SessionCheck::Gone);
        assert!(
            pool.begin(
                &Reservation {
                    session: stranger,
                    ..reservation
                },
                "opus",
                None,
                None
            )
            .is_err()
        );
        pool.message(&session, "Message about T.1: stop", true)
            .unwrap();
        assert!(pool.sent()[0].urgent);
        assert!(pool.slots("claude").is_empty());
        assert!(pool.held("claude").is_none());
    }
}
