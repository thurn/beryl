//! The integration tests of `beryl_host`, as one binary: each file under `tests/` used
//! to be a binary of its own, and every one of them linked the crate and its dependencies again
//! after each edit.

#[path = "../../../../test_data.rs"]
mod test_data;
#[path = "../../../../test_scans.rs"]
mod test_scans;

mod events;
mod faces;
mod frames;
mod icons;
mod paints;
mod phone;
mod threads;
mod tier;

mod asset_cards;
