use beryl_table::apply::Entry;
use beryl_table::permissions;
use beryl_table::state::{ObjectId, Zone};
use beryl_table::{Action, Chance, Log, Refusal, Seat, Table};

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
                after_move: None,
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
        self.apply_checked(table, log, by, action, chance, |_, _, _| Ok(()))
    }

    /// Passes the previous table, staged table and new entry to the publication
    /// check before committing the action.
    #[allow(clippy::too_many_arguments)] // Same inputs as apply, with a projection check.
    pub fn apply_checked<'log>(
        &self,
        table: &mut Table,
        log: &'log mut Log,
        by: Seat,
        action: TableAction,
        chance: &mut Chance,
        check: impl FnOnce(&Table, &Table, &Entry) -> Result<(), Refusal>,
    ) -> Result<&'log Entry, Refusal> {
        if by != self.seat {
            return Err(Refusal::Stale);
        }
        log.apply_mapped_checked(
            table,
            by,
            action,
            chance,
            |staged, action| self.resolve_action(staged, action),
            check,
        )
    }

    fn resolve_action(&self, staged: &Table, mut action: TableAction) -> Result<Action, Refusal> {
        if let Action::Arrow { selection, .. } = &mut action
            && selection
                .objects
                .iter()
                .any(|reference| reference.after_move == Some(true))
        {
            // The selection names the original hand. Use the staged destination
            // so the client need not predict where the host played the card.
            let mut destination = None;
            for reference in &mut selection.objects {
                let (id, published) = self.issued(staged, reference)?;
                let current = permissions::reference_zone(staged, self.seat, id)
                    .filter(|zone| matches!(zone, Zone::Battlefield | Zone::Stack))
                    .ok_or(Refusal::Stale)?;
                if reference.after_move != Some(true)
                    || !matches!(published, Zone::Hand(_))
                    || selection.zone != published
                    || destination.is_some_and(|zone| zone != current)
                {
                    return Err(Refusal::Stale);
                }
                destination = Some(current);
                reference.after_move = None;
            }
            selection.zone = destination.expect("selection contains a moved reference");
        }
        action.try_map_objects(&mut |reference, zone| {
            let (id, published) = self.issued(staged, &reference)?;
            if reference.after_move == Some(true)
                || (zone != published && !zone.is_public())
                || permissions::reference_zone(staged, self.seat, id) != Some(zone)
            {
                return Err(Refusal::Stale);
            }
            Ok(id)
        })
    }

    fn issued(
        &self,
        table: &Table,
        reference: &TableObjectRef,
    ) -> Result<(ObjectId, Zone), Refusal> {
        if self.revision != table.revision() || reference.view != self.view {
            return Err(Refusal::Stale);
        }
        let index = reference.handle as usize;
        let entry = self.actions.get(index).ok_or(Refusal::Stale)?;
        Ok((self.ids[index], entry.zone))
    }
}
