//! The commands running on this machine, in `ignite status`: one line for each verb still
//! running, with its handle, verb, ticket, tree, class, age and step (see `crate::steps`).

use std::sync::Arc;

use super::StatusSection;
use crate::steps::{Kind, Steps};

/// The `running` section of `ignite status`. It shows nothing while no verb runs.
pub struct RunningSection(pub Arc<Steps>);

impl StatusSection for RunningSection {
    fn title(&self) -> &'static str {
        "running"
    }

    fn lines(&self) -> Vec<String> {
        self.0
            .live()
            .iter()
            .filter(|activity| matches!(activity.kind(), Kind::Verb(_)))
            .map(|activity| activity.view())
            // `status` itself, and the waits that only wait on another verb, say nothing.
            .filter(|view| !matches!(view.name.as_str(), "status" | "wait" | "doctor"))
            .map(|view| view.line())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ignite_core::fakes::FakeClock;
    use ignite_core::ids::{Handle, TicketId, TreeId};
    use ignite_core::protocol::Class;
    use ignite_core::time::Timestamp;

    use super::*;
    use crate::steps::Work;

    #[test]
    fn a_running_landing_is_listed_with_its_ticket_tree_class_and_step() {
        let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
        let steps = Arc::new(Steps::new(clock.clone()));
        let landing = steps.begin(
            Kind::Verb("land".to_owned()),
            Work {
                class: Some(Class::Land),
                ticket: Some(TicketId::new("Tqtj2").unwrap()),
                session: None,
                tree: Some(TreeId::new("tree-04").unwrap()),
                tree_path: None,
            },
        );
        landing.attach_handle(&Handle::new("L-2").unwrap());
        clock.advance(600.0);
        landing.set_step("building rust-tests in landing-3");
        clock.advance(300.0);
        let _status = steps.begin(Kind::Verb("status".to_owned()), Work::default());
        let section = RunningSection(steps.clone());
        assert_eq!(
            section.lines(),
            ["L-2 land Tqtj2 tree-04 land for 15 min: building rust-tests in landing-3 (5 min)"]
        );
        landing.finish();
        assert!(section.lines().is_empty());
    }
}
