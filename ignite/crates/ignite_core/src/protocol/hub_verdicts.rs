//! Verdicts and reverted landings (see "Landing by verdict" in `docs/ignite.md`).
//!
//! A verdict records that the landing gate passed for one tree of files on one base. A landing
//! whose tree and base have a verdict pushes its commits, rebased onto the tip in git's object
//! database, without running the gate again. A tip round that finds a build check failing
//! because of one landing reverts it and reports a [`Reverted`].

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::ids::{CheckName, Commit, TicketId};
use crate::time::Timestamp;
use crate::tipround::CulpritSearch;

/// What a verdict is found by: the files, the base they were checked against, and the gate's
/// settings. It holds nothing about the daemon's build, so a verdict outlives Ignite's own
/// releases, which come every few minutes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VerdictKey {
    /// The git tree ID of every file in the ticket tree, committed or not.
    pub tree: String,
    /// The merge base of the tree's `HEAD` and the tip of the integration branch.
    pub base: Commit,
    /// [`verdict_settings`] of the configuration the gate ran under.
    pub settings: String,
}

/// `POST /verdicts/store`, and the reply to `POST /verdicts/lookup` (body [`VerdictKey`]): a
/// gate that passed with nothing incomplete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub key: VerdictKey,
    /// The tree's `HEAD` when the gate passed.
    pub head: Commit,
    /// The hostname of the machine that ran the gate.
    pub machine: String,
    pub passed_at: Timestamp,
    /// The check families the gate's scope held.
    pub families: Vec<String>,
    /// How many checks the gate left for tip verification.
    pub left_to_tip: u32,
    /// Families of the always set that the gate did not run, such as those no machine here can
    /// run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub always_not_run: Vec<String>,
}

/// `POST /reverted`: a tip round reverted one landing because a build check failed with it and
/// passed without it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reverted {
    /// The ticket that landed the commits; `None` for commits no landing recorded.
    pub ticket: Option<TicketId>,
    /// The reverted landing's commits, oldest first.
    pub landed: Vec<Commit>,
    /// The commit that undoes them, pushed to the integration branch.
    pub revert: Commit,
    /// The build checks that failed with the landing.
    pub checks: Vec<CheckName>,
    /// What the failing checks printed.
    pub output: String,
    /// How the round found the landing.
    pub search: CulpritSearch,
    /// The hostname of the machine that ran the round.
    pub machine: String,
}

/// The reply to `POST /reverted`: the reopened ticket, or the new P0 ticket for commits no
/// landing recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevertFiled {
    pub ticket: TicketId,
}

/// A digest of the `[gate]` section of `config`, the part of [`VerdictKey`] that changes when
/// the gate's values change. It is FNV-1a over the section serialized as JSON, the hash the
/// express lane uses for its own memory of passes: stable across runs and Rust versions, unlike
/// the standard library's hasher.
pub fn verdict_settings(config: &Config) -> String {
    let text = serde_json::to_string(&config.gate).unwrap_or_default();
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        digest ^= u64::from(byte);
        digest = digest.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{digest:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(c: char) -> Commit {
        Commit::new(c.to_string().repeat(40)).unwrap()
    }

    fn key() -> VerdictKey {
        VerdictKey {
            tree: "f".repeat(40),
            base: commit('a'),
            settings: "0123456789abcdef".to_owned(),
        }
    }

    #[test]
    fn a_verdict_round_trips_and_parses_without_its_optional_list() {
        let verdict = Verdict {
            key: key(),
            head: commit('b'),
            machine: "jarvis".to_owned(),
            passed_at: Timestamp::EPOCH,
            families: vec!["rust".to_owned()],
            left_to_tip: 12,
            always_not_run: vec!["windows-only".to_owned()],
        };
        let text = serde_json::to_string(&verdict).unwrap();
        assert_eq!(serde_json::from_str::<Verdict>(&text).unwrap(), verdict);

        let mut value = serde_json::to_value(&verdict).unwrap();
        value.as_object_mut().unwrap().remove("always_not_run");
        let parsed: Verdict = serde_json::from_value(value).unwrap();
        assert!(parsed.always_not_run.is_empty());
        let key_text = serde_json::to_string(&key()).unwrap();
        assert_eq!(
            serde_json::from_str::<VerdictKey>(&key_text).unwrap(),
            key()
        );
    }

    #[test]
    fn a_reverted_landing_round_trips() {
        let reverted = Reverted {
            ticket: Some(TicketId::new("T.1").unwrap()),
            landed: vec![commit('c'), commit('d')],
            revert: commit('e'),
            checks: vec!["clippy:workspace".parse().unwrap()],
            output: "error[E0425]".to_owned(),
            search: CulpritSearch::Bisected { runs: 2 },
            machine: "jarvis".to_owned(),
        };
        let text = serde_json::to_string(&reverted).unwrap();
        assert_eq!(serde_json::from_str::<Reverted>(&text).unwrap(), reverted);
        let filed = RevertFiled {
            ticket: TicketId::new("T.1").unwrap(),
        };
        let text = serde_json::to_string(&filed).unwrap();
        assert_eq!(serde_json::from_str::<RevertFiled>(&text).unwrap(), filed);
    }

    #[test]
    fn the_settings_digest_follows_the_gate_section_alone() {
        let text = crate::test_files::read_to_string("ignite.toml").unwrap();
        let config = Config::parse("ignite.toml", &text).unwrap();
        let digest = verdict_settings(&config);
        assert_eq!(digest.len(), 16);
        assert_eq!(verdict_settings(&config.clone()), digest);

        let mut other_machines = config.clone();
        other_machines.machine.clear();
        assert_eq!(verdict_settings(&other_machines), digest);

        let mut other_gate = config;
        other_gate.gate.threshold *= 2.0;
        assert_ne!(verdict_settings(&other_gate), digest);
    }
}
