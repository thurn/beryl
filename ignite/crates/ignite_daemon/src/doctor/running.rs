//! The commands running on this machine, in `ignite status`: one line for each verb still
//! running, with its handle, verb, the commit of the Ignite executable it runs under, ticket,
//! tree, class, age and step (see `crate::steps`). After the daemon restarts into a new build, a
//! verb in a verb process still runs under the build that started it, such as
//! `L-6 land under 87631b5 ...`.

use std::sync::Arc;

use super::StatusSection;
use crate::steps::{Kind, Steps};

/// The commit of the executable the verb with a handle runs under, if known.
pub type ExecutableOf = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The `running` section of `ignite status`, with where to read each handle's executable. It
/// shows nothing while no verb runs.
pub struct RunningSection(pub Arc<Steps>, pub Option<ExecutableOf>);

/// `line`, the view's line that starts with the handle and the verb, with ` under <commit>`
/// after the verb.
fn with_executable(line: &str, commit: &str) -> String {
    let short: String = commit.chars().take(9).collect();
    let mut words = line.splitn(3, ' ');
    match (words.next(), words.next(), words.next()) {
        (Some(handle), Some(verb), Some(rest)) => format!("{handle} {verb} under {short} {rest}"),
        _ => format!("{line} under {short}"),
    }
}

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
            .map(|view| {
                let line = view.line();
                let commit = view
                    .handle
                    .as_deref()
                    .zip(self.1.as_ref())
                    .and_then(|(handle, executable_of)| executable_of(handle));
                match commit {
                    Some(commit) => with_executable(&line, &commit),
                    None => line,
                }
            })
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
                caller_processes: std::collections::BTreeSet::new(),
            },
        );
        landing.attach_handle(&Handle::new("L-2").unwrap());
        clock.advance(600.0);
        landing.set_step("building rust-tests in landing-3");
        clock.advance(300.0);
        let _status = steps.begin(Kind::Verb("status".to_owned()), Work::default());
        let section = RunningSection(steps.clone(), None);
        assert_eq!(
            section.lines(),
            ["L-2 land Tqtj2 tree-04 land for 15 min: building rust-tests in landing-3 (5 min)"]
        );
        let executable_of: ExecutableOf = Arc::new(|handle| {
            (handle == "L-2").then(|| "87631b5a2c4e9f0d1b3a5c7e9f1a3b5c7d9e1f3a".to_owned())
        });
        let under = RunningSection(steps.clone(), Some(executable_of));
        assert_eq!(
            under.lines(),
            [
                "L-2 land under 87631b5a2 Tqtj2 tree-04 land for 15 min: building rust-tests in \
                 landing-3 (5 min)"
            ]
        );
        landing.finish();
        assert!(section.lines().is_empty());
    }
}
