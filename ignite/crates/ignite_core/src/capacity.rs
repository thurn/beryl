//! Capacity derived from machine measurements and reported to the hub.
//!
//! The tickets for derived core reserves, tree disk budgets and worker room implement
//! their respective functions here.

#![allow(unused_variables)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{Floor, Os};
use crate::ids::TicketId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineFacts {
    pub os: Os,
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub memory_mb: u64,
    pub disk_total_gb: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Derived {
    pub express_cores: u32,
    pub tip_build_cores: u32,
    pub floor: Floor,
    pub landing_trees: u32,
    pub worker_ceiling: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoomInputs {
    pub ceiling: u32,
    pub running: u32,
    pub memory_pressed: bool,
    pub free_memory_mb: Option<u64>,
    pub worker_mb: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CapacityReport {
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub memory_mb: u64,
    pub admit_cores: f64,
    pub free_cores: f64,
    pub worker_room: u32,
    pub free_memory_mb: Option<u64>,
    pub memory_pressed: bool,
    pub free_disk_gb: Option<u64>,
    pub disk_budget_gb: u64,
    pub queue_seconds: f64,
    /// Configured build output directories, whether or not any tree holds them.
    pub outputs: Vec<String>,
    pub free_trees: Vec<Vec<String>>,
    pub leased_trees: BTreeMap<TicketId, Vec<String>>,
    pub providers: Vec<ProviderCapacity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapacity {
    pub name: String,
    pub tiers: Vec<String>,
    pub probed: bool,
    pub hold_reason: Option<String>,
}

pub fn derive(facts: &MachineFacts) -> Derived {
    todo!()
}

pub fn disk_budget_gb(disk_total_gb: u64, outside_gb: u64, cap_gb: Option<u32>) -> u64 {
    todo!()
}

pub fn start_room(inputs: &RoomInputs) -> u32 {
    todo!()
}
