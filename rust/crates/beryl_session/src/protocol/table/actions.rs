use beryl_table::apply::Entry;
use beryl_table::permissions;
use beryl_table::state::ObjectId;
use beryl_table::{Chance, Log, Refusal, Seat, Table};

use super::{TableAction, TableObjectAction, TableObjectRef};
use crate::script::Subject;

/// Host-only mapping for a published view. Keep one active mapping per authenticated
/// seat and replace it atomically with the view. The same mapping serves local and
/// linked commands. A reconnect or a new table must publish a fresh mapping.
pub struct TableActionRefs {
    seat: Seat,
    revision: u32,
    view: String,
    ids: Vec<ObjectId>,
    actions: Vec<TableObjectAction>,
}

impl TableActionRefs {
    pub fn new(table: &Table, seat: Seat) -> Self {
        Self {
            seat,
            revision: table.revision(),
            view: uuid::Uuid::new_v4().simple().to_string(),
            ids: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Register only items the adapter actually projected to this viewer (including
    /// anonymous card backs they can use). Pass the animation subject for a marker
    /// and the projected slot for a card. Call once per displayed subject.
    pub fn expose(&mut self, table: &Table, subject: Subject, id: ObjectId) {
        let distinct_kind = match subject {
            Subject::Object { .. } => table.objects.contains_key(&id),
            Subject::Ability { id: marker } => marker == id.0 && table.markers.contains_key(&id),
            Subject::Player { .. } => false,
        };
        if !distinct_kind || self.revision != table.revision() {
            return;
        }
        let Some(zone) = permissions::reference_zone(table, self.seat, id) else {
            return;
        };
        let handle = u32::try_from(self.ids.len()).expect("at most one handle per object");
        self.ids.push(id);
        self.actions.push(TableObjectAction {
            subject,
            reference: TableObjectRef {
                view: self.view.clone(),
                handle,
            },
            zone,
        });
    }

    /// Entries to copy into the published `TableView.object_actions`.
    pub fn actions(&self) -> &[TableObjectAction] {
        &self.actions
    }

    /// Resolve against the authenticated seat's current mapping, then use the
    /// normal permission checks and transactional log application for the action.
    pub fn apply<'log>(
        &self,
        table: &mut Table,
        log: &'log mut Log,
        by: Seat,
        action: TableAction,
        chance: &mut Chance,
    ) -> Result<&'log Entry, Refusal> {
        self.apply_checked(table, log, by, action, chance, |_, _| Ok(()))
    }

    /// Checks the action's result before the new table and log become authoritative.
    #[allow(clippy::too_many_arguments)] // Same inputs as apply, with a projection check.
    pub fn apply_checked<'log>(
        &self,
        table: &mut Table,
        log: &'log mut Log,
        by: Seat,
        action: TableAction,
        chance: &mut Chance,
        check: impl FnOnce(&Table, &Entry) -> Result<(), Refusal>,
    ) -> Result<&'log Entry, Refusal> {
        if by != self.seat {
            return Err(Refusal::Stale);
        }
        let action = action.try_map_objects(&mut |reference, zone| {
            if self.revision != table.revision() || reference.view != self.view {
                return Err(Refusal::Stale);
            }
            let index = reference.handle as usize;
            let entry = self.actions.get(index).ok_or(Refusal::Stale)?;
            if zone != entry.zone {
                return Err(Refusal::Stale);
            }
            let id = self.ids[index];
            if table
                .position(id)
                .is_none_or(|(current, _)| current != zone)
            {
                return Err(Refusal::Stale);
            }
            Ok(id)
        })?;
        log.apply_checked(table, by, &action, chance, check)
    }
}
