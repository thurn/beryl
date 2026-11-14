use crate::{Action, Refusal, Seat, Table};

// TODO(permissions): check the actor's access to the objects and zones the action names.
pub fn check(_table: &Table, _by: Seat, _action: &Action) -> Result<(), Refusal> {
    Ok(())
}
