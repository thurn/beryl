//! Game-owned roles, state bits and annotation slots.

use kit_frame::frame::Role;

/// An ability on the stack or in an ability-ordering prompt.
pub const ABILITY_ROLE: Role = Role::Custom("ability");

pub const TAPPED: u32 = 1 << 0;
pub const PHASED_OUT: u32 = 1 << 10;
pub const PICKED: u32 = 1 << 11;

pub const COPY_NOTE: usize = 0;
pub const FACE_NOTE: usize = 1;
