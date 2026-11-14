//! Runs the manual table and publishes each accepted action's view on the game feed.

use std::hash::{BuildHasher, RandomState};
use std::sync::{Mutex, MutexGuard, PoisonError};

use beryl_session::protocol::{TableAction, TableActionRefs};
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table::deck::Deck;
use beryl_table::{Action, Chance, Log, Refusal, Seat, Table, redact};

use crate::script::script;
use crate::view::{Slots, view};

const SOLO: Seat = Seat(0);

pub struct TableSessions {
    ids: SessionIds,
    // Serialize actions and their publication, so two callers can't emit views out of order.
    current: Mutex<Option<Solo>>,
    emit: Box<dyn Fn(GameUpdate) + Send + Sync>,
}

struct Solo {
    table: Table,
    log: Log,
    chance: Chance,
    slots: Slots,
    refs: TableActionRefs,
    update: GameUpdate,
}

impl TableSessions {
    /// Use the engine's `SessionIds` if both sessions send to the same stage.
    /// The emitter sends updates in order and must not call back into this session.
    pub fn new(ids: SessionIds, emit: impl Fn(GameUpdate) + Send + Sync + 'static) -> Self {
        Self {
            ids,
            current: Mutex::default(),
            emit: Box::new(emit),
        }
    }

    /// Start a table with one seat, replacing any table after its new view is ready.
    pub fn solo(&self, deck: Deck) -> Result<SessionId, Refusal> {
        self.solo_seeded(deck, RandomState::new().hash_one(0_u8))
    }

    /// Start a solo table with a repeatable shuffle for a recorded run.
    pub fn solo_seeded(&self, deck: Deck, seed: u64) -> Result<SessionId, Refusal> {
        let mut current = self.lock();
        let mut table = Table::new(1);
        let mut log = Log::new(&table);
        let mut chance = Chance::fresh(seed);
        log.apply(
            &mut table,
            SOLO,
            &Action::LoadDeck {
                seat: SOLO,
                main: deck.main,
                side: deck.side,
            },
            &mut chance,
        )?;
        let table_before = table.clone();
        let mut slots = Slots::default();
        let (before, _) = view(&table_before, &log, SOLO, &mut slots)?;
        log.apply(
            &mut table,
            SOLO,
            &Action::Start { first: SOLO },
            &mut chance,
        )?;
        let mut next_slots = slots.clone();
        let (mut view, refs) = view(&table, &log, SOLO, &mut next_slots)?;
        view.script = Some(script(
            &before,
            log.entries().last().expect("start recorded an entry"),
            &table_before,
            &view,
            SOLO,
            &slots,
            &next_slots,
        ));
        let session = self.ids.allocate();
        let update = GameUpdate { session, view };
        *current = Some(Solo {
            table,
            log,
            chance,
            slots: next_slots,
            refs,
            update: update.clone(),
        });
        (self.emit)(update);
        Ok(session)
    }

    pub fn act(&self, action: TableAction) -> Result<(), Refusal> {
        let mut current = self.lock();
        let solo = current.as_mut().ok_or(Refusal::Stale)?;
        let table_before = solo.table.clone();
        let mut slots = solo.slots.clone();
        solo.refs.apply_checked(
            &mut solo.table,
            &mut solo.log,
            SOLO,
            action,
            &mut solo.chance,
            |table, entry| {
                // Refuse an action that would exhaust the stage's slots before committing it.
                let projected = redact::seen_by(table, SOLO);
                slots
                    .update(&projected, SOLO, std::slice::from_ref(entry))
                    .ok_or(Refusal::INVALID)
            },
        )?;
        let (mut view, refs) = view(&solo.table, &solo.log, SOLO, &mut slots)
            .expect("the action checked the next view's slots");
        view.script = Some(script(
            &solo.update.view,
            solo.log
                .entries()
                .last()
                .expect("the action recorded an entry"),
            &table_before,
            &view,
            SOLO,
            &solo.slots,
            &slots,
        ));
        solo.slots = slots;
        solo.refs = refs;
        solo.update.view = view;
        (self.emit)(solo.update.clone());
        Ok(())
    }

    pub fn current(&self) -> Option<GameUpdate> {
        self.lock().as_ref().map(|solo| solo.update.clone())
    }

    pub fn leave(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> MutexGuard<'_, Option<Solo>> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
