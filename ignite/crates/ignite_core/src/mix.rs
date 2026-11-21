//! Provider selection at reservation time.
//!
//! The ticket that chooses each reservation's provider implements the selection here.

#![allow(unused_variables)]

use crate::fleet::ProviderSetting;
use crate::plan_usage::Hold;
use crate::protocol::hub::ProviderOffer;

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderLoad {
    pub name: String,
    pub setting: ProviderSetting,
    pub in_progress: u32,
    pub holds: Vec<Hold>,
}

pub fn choose<'a>(
    loads: &'a [ProviderLoad],
    offered: &[ProviderOffer],
    tier: Option<&str>,
    pinned: Option<&str>,
) -> Result<&'a str, String> {
    todo!()
}
