//! Giving a slot to a new session: which slot it takes, what a CLI session that worked a
//! ticket here before brings to its next one (its slot number, turn count and transcript), and
//! what a ticket that landed leaves for a regression it causes.
//!
//! A regression goes first to the worker whose ticket caused it, because that worker still
//! holds the change's context. A CLI session ends when its ticket lands, so the context is kept
//! another way: when a slot is freed because its ticket is no longer held, the ticket, app,
//! model and transcript go into `cli_landed` ([`CliDaemon::record_landed`]). For 24 hours, a
//! new session of the same app whose hand-off names that ticket as its culprit resumes the
//! transcript, under its own Ignite session ID, with the regression's hand-off prompt and the
//! regression's model and effort ([`CliDaemon::resume_culprit`]). A record is used once.
//!
//! The transcript's files belong to the CLI, which may prune them, so a resume can fail. The
//! slot then keeps the plain hand-off prompt in `fallback_prompt`: if the resumed first turn
//! ends as anything but a clean turn, the ticket starts again at once on a new transcript (see
//! `end`), so a failed resume costs the regression one turn start and nothing more.

use ignite_core::cli::{SlotState, TurnEnd};
use ignite_core::ids::SessionId;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::{CliLanded, CliSlot};

use super::{CliDaemon, CliPool, LANDED_KEEP_S, Resume, hex, random_bytes, slot_of};

/// How many slots [`CliDaemon::open_slot`] tries before it gives up: each failed try means
/// another hand-off took the slot first.
const OPEN_ATTEMPTS: u32 = 4;

/// A CLI session that worked a ticket here before, as its turns recorded it.
pub(super) struct Earlier {
    pub(super) app: String,
    pub(super) index: u32,
    turn: i64,
    transcript: Option<String>,
}

impl super::CliAppSetup {
    /// Older workers could store an empty model for a ticket without a tier. Use the same
    /// configured default for those workers when they retry and for new untiered hand-offs.
    pub(super) fn model_for<'a>(&'a self, model: Option<&'a str>) -> Result<&'a str> {
        model
            .filter(|model| !model.is_empty())
            .or(self.default_model.as_deref())
            .ok_or_else(|| {
                Error::Invalid(format!("no CLI model configured for {}", self.app.name()))
            })
    }
}

impl CliDaemon {
    /// Checks the hub before a continuation after a hold, including when a daemon restart
    /// left the next turn recorded without a supervisor. The old local lease can outlive
    /// the hub's claim.
    pub(super) fn retire_if_claim_lost(&self, row: &CliSlot) -> Result<bool> {
        if !super::health::parked_by_hold(row.park_reason.as_deref()) {
            return Ok(false);
        }
        let slot = slot_of(row);
        if let (Some(session), Some(ticket)) = (&slot.session, &slot.ticket)
            && !self.effects.claim_is_current(session, ticket)?
        {
            if slot.state == SlotState::Starting
                && let Some(turn) = self.turn_row(session, slot.turn)?
            {
                self.close(
                    row,
                    &turn,
                    &TurnEnd::Interrupted,
                    None,
                    SlotState::Free,
                    |_| {},
                )?;
            } else {
                self.retire_slot(row, session)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Records and starts the next turn with its prompt and every undelivered message.
    pub(super) fn start_next(&self, row: &CliSlot) -> Result<()> {
        if self.held(&row.app).is_some() || self.retire_if_claim_lost(row)? {
            return Ok(());
        }
        let Some((turn, deliver)) = self.next_turn(row)? else {
            return Ok(());
        };
        self.database(|conn| store::record_turn_start(conn, &turn, &deliver))?;
        let mut row = row.clone();
        row.turn = turn.turn;
        SlotState::Starting.as_str().clone_into(&mut row.state);
        self.launch(&row)
    }

    /// What the turns of `session`, a CLI session whose slot has been freed, say about it;
    /// `None` when it never ran a turn here. A session that still holds a slot cannot take
    /// another ticket until its turn has ended and the slot is free. With `fresh`, the next
    /// turn starts a new transcript.
    pub(super) fn earlier_session(
        &self,
        session: &SessionId,
        fresh: bool,
    ) -> Result<Option<Earlier>> {
        if self.row(session)?.is_some() {
            return Err(Error::Conflict(format!(
                "{session} is still finishing its last ticket's turn"
            )));
        }
        let turns = self.database(|conn| store::turns(conn, session.as_str()))?;
        let Some(last) = turns.last() else {
            return Ok(None);
        };
        Ok(Some(Earlier {
            app: last.app.clone(),
            index: u32::try_from(last.slot).unwrap_or(0),
            turn: last.turn,
            transcript: if fresh {
                None
            } else {
                turns.iter().rev().find_map(|t| t.transcript.clone())
            },
        }))
    }

    /// Records that the ticket of `row`, a slot about to be freed because its ticket is no
    /// longer held, landed with the transcript its turns named. Records older than
    /// [`LANDED_KEEP_S`] are deleted. A slot with no ticket or no transcript leaves nothing.
    pub(super) fn record_landed(&self, row: &CliSlot) -> Result<()> {
        let (Some(ticket), Some(transcript)) = (&row.ticket, &row.transcript) else {
            return Ok(());
        };
        let now = self.now();
        let landed = CliLanded {
            ticket: ticket.clone(),
            app: row.app.clone(),
            transcript: Some(transcript.clone()),
            model: row.model.clone(),
            landed_at: Some(now.to_string()),
        };
        let expired_before = now.plus_seconds(-LANDED_KEEP_S).to_string();
        self.database(|conn| store::record_landed(conn, &landed, &expired_before))
    }

    /// `row`, a new session's slot, changed to resume the transcript of `resume.culprit` with
    /// `resume.prompt` when that ticket landed from a slot of the same app within
    /// [`LANDED_KEEP_S`]; otherwise `row` as it is. A resumed slot keeps its plain hand-off
    /// prompt as the fallback for a first turn that does not end cleanly.
    pub(super) fn resume_culprit(&self, row: CliSlot, resume: &Resume) -> Result<CliSlot> {
        let landed = self.database(|conn| store::take_landed(conn, resume.culprit.as_str()))?;
        let Some(landed) = landed else {
            return Ok(row);
        };
        let oldest = self.now().plus_seconds(-LANDED_KEEP_S);
        let recent = landed
            .landed_at
            .as_deref()
            .and_then(|t| Timestamp::parse(t).ok())
            .is_some_and(|at| at >= oldest);
        match landed.transcript {
            Some(transcript) if recent && landed.app == row.app => Ok(CliSlot {
                transcript: Some(transcript),
                fallback_prompt: row.next_prompt.clone(),
                next_prompt: Some(resume.prompt.clone()),
                ..row
            }),
            _ => Ok(row),
        }
    }

    /// Slot `preferred` of `app` if it is free, or else the lowest free one, or else one
    /// numbered past every slot there is.
    fn free_index(&self, app: &str, preferred: u32) -> Result<i64> {
        let rows: Vec<CliSlot> = self.rows()?.into_iter().filter(|r| r.app == app).collect();
        let busy = |index: i64| rows.iter().any(|r| r.slot == index && r.session.is_some());
        let preferred = i64::from(preferred);
        if preferred > 0 && !busy(preferred) {
            return Ok(preferred);
        }
        // Among one more number than there are rows, one is always free.
        let last = i64::try_from(rows.len()).unwrap_or(i64::MAX - 1) + 1;
        Ok((1..=last).find(|index| !busy(*index)).unwrap_or(last))
    }

    /// Writes `row`, a slot given to a new session by [`CliDaemon::claim_slot`], and records
    /// its first turn in the same transaction, then starts that turn. Unless the app is on a
    /// CLI hold: then the slot waits, backing off, for the hold to end.
    ///
    /// The slot is written only if it is still free. The dispatcher hands tickets over from
    /// more than one thread, and the slot's row is keyed by app and number, so a write over
    /// another session's slot would take it from that session and strand its turn. The
    /// caller holds `opening`, so within one daemon no other hand-off can take the slot
    /// meanwhile; should anything else have, this one takes the lowest free slot instead.
    ///
    /// Once the turn is recorded, the session exists whether or not its supervisor starts: a
    /// supervisor that fails to start is started by the `ignite-cli` loop, which finds the
    /// turn recorded and not started. So that failure is logged, not returned, and the
    /// dispatcher adds the session to the pool as it does for any other start.
    pub(super) fn open_slot(&self, mut row: CliSlot) -> Result<()> {
        for _ in 0..OPEN_ATTEMPTS {
            let first = match self.held(&row.app) {
                Some(_) => None,
                None => self.next_turn(&row)?,
            };
            let claimed = self.database(|conn| {
                store::claim(conn, &row, first.as_ref().map(|(t, d)| (t, d.as_slice())))
            })?;
            if !claimed {
                row.slot = self.free_index(&row.app, 0)?;
                continue;
            }
            if let Some((turn, _)) = first {
                row.turn = turn.turn;
                SlotState::Starting.as_str().clone_into(&mut row.state);
                if let Err(error) = self.launch(&row) {
                    eprintln!(
                        "ignite: turn {} of {} recorded; its supervisor starts later: {error}",
                        turn.turn, turn.session
                    );
                }
            }
            return Ok(());
        }
        Err(Error::Conflict(format!(
            "no free slot of {} could be taken for {}",
            row.app,
            row.session.unwrap_or_default()
        )))
    }

    /// A new Ignite session for a CLI worker of `app`, `cli-<app>-<8 hex>`.
    pub(super) fn new_session(app: &str) -> Result<SessionId> {
        SessionId::new(format!("cli-{app}-{}", hex(&random_bytes::<4>()?)))
            .map_err(|e| Error::Invalid(e.to_string()))
    }

    /// The row of a slot of `app` given to `session`, not yet written: slot `preferred` if it
    /// is free, or else the lowest free one, or else one numbered past every slot there is. Its
    /// first turn waits to start with `prompt`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn claim_slot(
        &self,
        app: &str,
        preferred: u32,
        session: &SessionId,
        model: &str,
        effort: Option<&str>,
        prompt: String,
        earlier: Option<Earlier>,
    ) -> Result<CliSlot> {
        let index = self.free_index(app, preferred)?;
        let now = self.now().to_string();
        Ok(CliSlot {
            app: app.to_owned(),
            slot: index,
            session: Some(session.to_string()),
            ticket: None,
            state: SlotState::BackingOff.as_str().to_owned(),
            turn: earlier.as_ref().map_or(0, |e| e.turn),
            transcript: earlier.and_then(|e| e.transcript),
            model: Some(model.to_owned()),
            effort: effort.map(str::to_owned),
            since: Some(now.clone()),
            reservation: None,
            next_prompt: Some(prompt),
            wait_handle: None,
            resume_at: Some(now),
            park_reason: None,
            interrupted_in_row: 0,
            block_sent: 0,
            backed_off: 0,
            fallback_prompt: None,
        })
    }
}
