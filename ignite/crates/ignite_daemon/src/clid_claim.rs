//! Giving a slot to a new session: which slot it takes, and what a CLI session that worked a
//! ticket here before brings to its next one (its slot number, turn count and transcript).

use ignite_core::cli::SlotState;
use ignite_core::ids::SessionId;
use ignite_core::traits::{Error, Result};
use ignite_store::local::cli_slots as store;
use ignite_store::local::rows::CliSlot;

use super::{CliDaemon, hex, random_bytes};

/// A CLI session that worked a ticket here before, as its turns recorded it.
pub(super) struct Earlier {
    pub(super) app: String,
    pub(super) index: u32,
    turn: i64,
    transcript: Option<String>,
}

impl CliDaemon {
    /// What the turns of `session`, a CLI session whose slot has been freed, say about it;
    /// `None` when it never ran a turn here. A session that still holds a slot cannot take
    /// another ticket until its turn has ended and the slot is free.
    pub(super) fn earlier_session(&self, session: &SessionId) -> Result<Option<Earlier>> {
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
            transcript: turns.iter().rev().find_map(|t| t.transcript.clone()),
        }))
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
        let rows: Vec<CliSlot> = self.rows()?.into_iter().filter(|r| r.app == app).collect();
        let busy = |index: i64| rows.iter().any(|r| r.slot == index && r.session.is_some());
        let preferred = i64::from(preferred);
        let index = if preferred > 0 && !busy(preferred) {
            preferred
        } else {
            // Among one more number than there are rows, one is always free.
            let last = i64::try_from(rows.len()).unwrap_or(i64::MAX - 1) + 1;
            (1..=last).find(|index| !busy(*index)).unwrap_or(last)
        };
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
        })
    }
}
