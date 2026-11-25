//! Game-owned roles, state bits and annotation slots.

use kit_frame::frame::Role;

/// An ability on the stack or in an ability-ordering prompt.
pub const ABILITY_ROLE: Role = Role::Custom("ability");

pub const TAPPED: u32 = 1 << 0;
pub const PHASED_OUT: u32 = 1 << 10;
pub const PICKED: u32 = 1 << 11;
/// A battlefield card its row lays partly over or under another on purpose: a copy behind a
/// pile's top card, a card tucked under its host, or any card of a row so crowded that its cards
/// overlap at their least size. A card on its way along a script's track wears it too, since
/// its row does not lay it out until the track settles.
pub const OVERLAPPED: u32 = 1 << 21;

pub const COPY_NOTE: usize = 0;
pub const FACE_NOTE: usize = 1;
