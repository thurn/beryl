//! The faults the interface tests are allowed to have today: `data/beryl3d/test-faults.json`,
//! `{"tests": {"<test name>": {"<rule word>": <count>}}}`.
//!
//! Beryl 3D's interface is built in milestones, and some interface checks fail on a recording
//! until the milestone that builds the verb, mark or placement it shows. The list lets
//! `just b3d test` pass today while still failing for any fault that is not on it, and for a
//! count that is over its entry. An entry whose count is now too high also fails, with the
//! command that lowers it, so that a fix is recorded in the commit that makes it and no room is
//! left for the fault to come back unseen.

use std::collections::BTreeMap;
use std::path::PathBuf;

use beryl_scene::agent::Fault;

use crate::recordings;
use crate::runner::Outcome;

/// The count of faults of each rule, by the rule's word.
pub type Counts = BTreeMap<String, usize>;

/// The listed faults of every test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    /// Test name to the count of faults of each rule. No entry is empty.
    pub tests: BTreeMap<String, Counts>,
}

/// How a run's faults compare with the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Comparison {
    /// Each fault count over its entry or not listed at all: the test's name, then a sentence.
    pub over: Vec<(String, String)>,
    /// Each entry that is now too high: the test's name, then a sentence.
    pub high: Vec<(String, String)>,
}

/// What an update did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub lowered: usize,
    pub removed: usize,
    pub raised: usize,
    pub added: usize,
}

impl Changes {
    #[must_use]
    pub fn any(&self) -> bool {
        self.lowered + self.removed + self.raised + self.added > 0
    }
}

/// The count of faults of each rule in `faults`.
#[must_use]
pub fn counts(faults: &[Fault]) -> Counts {
    let mut counts = Counts::new();
    for fault in faults {
        *counts.entry(fault.rule.word().to_owned()).or_default() += 1;
    }
    counts
}

/// The environment variable that names another list than `data/beryl3d/test-faults.json`, for
/// the tests of `test` itself.
pub const PATH_VARIABLE: &str = "BERYL_TEST_FAULTS";

/// The path of `data/beryl3d/test-faults.json`, or of the list [`PATH_VARIABLE`] names.
#[must_use]
pub fn path() -> PathBuf {
    if let Some(path) = std::env::var_os(PATH_VARIABLE) {
        return PathBuf::from(path);
    }
    recordings::root().join("data/beryl3d/test-faults.json")
}

impl Listed {
    /// The list as the file holds it.
    ///
    /// # Errors
    ///
    /// When the file cannot be read or is not the list.
    pub fn read() -> Result<Self, String> {
        let path = path();
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The list in `text`.
    ///
    /// # Errors
    ///
    /// When `text` is not `{"tests": {name: {rule: count}}}`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let tests = serde_json::from_value(value["tests"].clone())
            .map_err(|e| format!("`tests` is not a map of counts by rule: {e}"))?;
        Ok(Self { tests })
    }

    /// The file's text, which sorts tests and rules by name.
    #[must_use]
    pub fn text(&self) -> String {
        let value = serde_json::json!({ "tests": self.tests });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    }

    /// Writes the list to the file.
    ///
    /// # Errors
    ///
    /// When the file cannot be written.
    pub fn write(&self) -> Result<(), String> {
        let path = path();
        std::fs::write(&path, self.text()).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// How the faults of `outcomes` compare with the list. A test that could not run is left
    /// out. When `every_test` is true the outcomes are all the tests, so an entry for a test
    /// that did not run is too high; otherwise only the tests that ran are held to their
    /// entries. `update` is the command that lowers an entry.
    #[must_use]
    pub fn compare(&self, outcomes: &[Outcome], every_test: bool, update: &str) -> Comparison {
        let mut comparison = Comparison::default();
        let no_entry = Counts::new();
        for outcome in outcomes.iter().filter(|o| o.error.is_none()) {
            let name = outcome.test.name();
            let found = counts(&outcome.faults);
            let entry = self.tests.get(&name).unwrap_or(&no_entry);
            for (rule, &n) in &found {
                match entry.get(rule) {
                    None => comparison.over.push((
                        name.clone(),
                        format!("{name} has {n} `{rule}` faults, and none are listed"),
                    )),
                    Some(&listed) if n > listed => comparison.over.push((
                        name.clone(),
                        format!(
                            "{name} has {n} `{rule}` faults, over the {listed} listed in \
                             data/beryl3d/test-faults.json"
                        ),
                    )),
                    Some(_) => {}
                }
            }
            for (rule, &listed) in entry {
                let n = found.get(rule).copied().unwrap_or(0);
                if n < listed {
                    comparison.high.push((
                        name.clone(),
                        format!(
                            "{name} has {n} `{rule}` faults, under the {listed} listed; \
                             lower the entry with `{update}`"
                        ),
                    ));
                }
            }
        }
        if every_test {
            for (name, entry) in &self.tests {
                if !outcomes.iter().any(|o| o.test.name() == *name) {
                    comparison.high.push((
                        name.clone(),
                        format!(
                            "{name} is listed with {} faults but is not a test; remove the \
                             entry with `{update}`",
                            entry.values().sum::<usize>()
                        ),
                    ));
                }
            }
        }
        comparison
    }

    /// The faults of `outcome` that the list does not allow: for each rule, those after the
    /// first as many as are listed.
    #[must_use]
    pub fn excess(&self, outcome: &Outcome) -> Vec<Fault> {
        let mut allowed = self
            .tests
            .get(&outcome.test.name())
            .cloned()
            .unwrap_or_default();
        outcome
            .faults
            .iter()
            .filter(|fault| match allowed.get_mut(fault.rule.word()) {
                Some(left) if *left > 0 => {
                    *left -= 1;
                    false
                }
                _ => true,
            })
            .cloned()
            .collect()
    }

    /// Lowers every entry that is too high to what `outcomes` found, and removes those that
    /// reach zero (and, when `every_test`, those of tests that do not exist). With `raise` it
    /// also raises an entry that is too low and adds a fault that is not listed.
    pub fn update(&mut self, outcomes: &[Outcome], every_test: bool, raise: bool) -> Changes {
        let mut changes = Changes::default();
        for outcome in outcomes.iter().filter(|o| o.error.is_none()) {
            let name = outcome.test.name();
            let found = counts(&outcome.faults);
            let mut entry = self.tests.remove(&name).unwrap_or_default();
            for (rule, &n) in &found {
                match entry.get(rule).copied() {
                    None if raise => {
                        entry.insert(rule.clone(), n);
                        changes.added += 1;
                    }
                    Some(listed) if n > listed && raise => {
                        entry.insert(rule.clone(), n);
                        changes.raised += 1;
                    }
                    Some(listed) if n < listed => {
                        entry.insert(rule.clone(), n);
                        changes.lowered += 1;
                    }
                    _ => {}
                }
            }
            let gone: Vec<String> = entry
                .keys()
                .filter(|rule| !found.contains_key(*rule))
                .cloned()
                .collect();
            for rule in gone {
                entry.remove(&rule);
                changes.removed += 1;
            }
            if !entry.is_empty() {
                self.tests.insert(name, entry);
            }
        }
        if every_test {
            let ran: Vec<String> = outcomes.iter().map(|o| o.test.name()).collect();
            let before = self.tests.len();
            self.tests.retain(|name, _| ran.contains(name));
            changes.removed += before - self.tests.len();
        }
        changes
    }

    /// How many tests have a listed fault, and the rules with the most listed faults, most
    /// first, at most `most` of them.
    #[must_use]
    pub fn summary(&self, most: usize) -> (usize, Vec<(String, usize)>) {
        let mut by_rule = Counts::new();
        for entry in self.tests.values() {
            for (rule, n) in entry {
                *by_rule.entry(rule.clone()).or_default() += n;
            }
        }
        let mut rules: Vec<(String, usize)> = by_rule.into_iter().collect();
        rules.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        rules.truncate(most);
        (self.tests.len(), rules)
    }
}
