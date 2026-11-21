//! The core readings used at startup and by `doctor`, and the retired settings it reports.

use ignite_core::capacity::MachineFacts;
use ignite_core::config::{Config, Os};

use super::{Finding, Host};

pub(crate) fn machine_facts(host: &dyn Host, os: Os) -> MachineFacts {
    MachineFacts {
        os,
        logical_cores: host.cores().max(1),
        physical_cores: host.physical_cores(),
        memory_mb: host.memory_mb(),
        disk_total_gb: 0, // Only core-based capacity is derived here.
    }
}

pub(super) fn retired_findings(config: &Config, findings: &mut Vec<Finding>) {
    for (name, machine) in &config.machine {
        let keys = [
            ("landing_trees", machine.landing_trees.is_some()),
            ("floor", machine.floor.is_some()),
            ("express_cores", machine.express_cores.is_some()),
            ("tip_build_cores", machine.tip_build_cores.is_some()),
        ];
        let resources = machine
            .resources
            .keys()
            .filter(|key| config.resources.contains_key(*key))
            .map(|key| format!("resources.{key}"));
        for key in keys
            .into_iter()
            .filter(|(_, present)| *present)
            .map(|(key, _)| key.to_owned())
            .chain(resources)
        {
            findings.push(Finding::ok(
                "config",
                format!(
                    "machine.{name}.{key} in ignite.toml is retired. Ignite uses the derived value."
                ),
            ));
        }
    }
}
