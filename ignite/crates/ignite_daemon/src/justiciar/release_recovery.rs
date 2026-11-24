//! Recovery from a release that failed its selftest or crash-looped at startup. The evidence
//! survives restarts because the daemon that reports the failure cannot also start its replacement.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use ignite_core::ids::Commit;
use ignite_core::protocol::hub::{JusticiarCleared, JusticiarRecovery, JusticiarReport};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Git, Hub, Result};

use crate::crash::CrashRecord;
use crate::update::{HEALTHY_AFTER_S, Home, StartRecord, start_log};

/// How many consecutive starts of descendants of the rejected release demonstrate recovery.
const CLEAN_STARTS: usize = 3;

#[derive(Serialize, Deserialize)]
struct ReleaseFault {
    commit: Commit,
    failed_at: Timestamp,
    report: JusticiarReport,
    filed: bool,
}

impl ReleaseFault {
    fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let partial = path.with_extension("partial");
        fs::write(&partial, serde_json::to_vec(self)?)?;
        fs::rename(partial, path)
    }
}

fn directory(home: &Path) -> PathBuf {
    home.join("justiciar-releases")
}

fn reports(home: &Path) -> impl Iterator<Item = (PathBuf, ReleaseFault)> {
    fs::read_dir(directory(home))
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let fault = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            Some((path, fault))
        })
}

pub(crate) fn queue_selftest(
    home: &Path,
    commit: &Commit,
    failed_at: Timestamp,
    report: JusticiarReport,
) -> io::Result<()> {
    let path = directory(home).join(format!("selftest-{commit}.json"));
    ReleaseFault {
        commit: commit.clone(),
        failed_at,
        report,
        filed: false,
    }
    .save(&path)
}

pub(crate) fn report_crash(
    home: &Path,
    hub: &dyn Hub,
    report: JusticiarReport,
    record: &CrashRecord,
) -> Result<()> {
    let commit = Commit::new(&record.commit).map_err(|error| Error::Invalid(error.to_string()))?;
    let failed_at =
        Timestamp::parse(&record.at).map_err(|error| Error::Invalid(error.to_string()))?;
    let path = directory(home).join(format!("crash-{}", report.evidence));
    let mut fault = ReleaseFault {
        commit,
        failed_at,
        report,
        filed: false,
    };
    file(hub, &path, &mut fault)
}

fn file(hub: &dyn Hub, path: &Path, fault: &mut ReleaseFault) -> Result<()> {
    // The hub deduplicates by evidence if it took the report before a failed save.
    hub.justiciar(&fault.report)?;
    fault.filed = true;
    fault.save(path)?;
    Ok(())
}

pub(crate) fn send_pending(home: &Path, hub: &dyn Hub) {
    for (path, mut fault) in reports(home).filter(|(_, fault)| !fault.filed) {
        if let Err(error) = file(hub, &path, &mut fault) {
            eprintln!("ignite: cannot report {}: {error}", fault.report.subject);
        }
    }
}

pub(super) fn send_cleared(home: &Path, git: &dyn Git, hub: &dyn Hub, now: Timestamp, pid: u32) {
    let mut filed = reports(home).filter(|(_, fault)| fault.filed).peekable();
    if filed.peek().is_none() {
        return;
    }
    let starts = start_log(&Home {
        root: home.to_owned(),
    })
    .read();
    for (path, fault) in filed {
        if !recovered(&fault, &starts, git, now, pid) {
            continue;
        }
        let report = JusticiarCleared {
            subject: fault.report.subject,
            recovery: JusticiarRecovery::Sighting {
                evidence: fault.report.evidence,
            },
        };
        let sent = hub
            .justiciar_cleared(&report)
            .and_then(|()| fs::remove_file(&path).map_err(Into::into));
        if let Err(error) = sent {
            eprintln!("ignite: cannot clear {}: {error}", report.subject);
        }
    }
}

/// Require a currently healthy run and two earlier runs that stopped cleanly. A rollback,
/// unrelated build or unclean stop breaks the sequence. An adopted release is recorded on
/// the same start and does not count as another start.
fn recovered(
    fault: &ReleaseFault,
    starts: &[StartRecord],
    git: &dyn Git,
    now: Timestamp,
    pid: u32,
) -> bool {
    let Some(current) = starts.last() else {
        return false;
    };
    if current.pid != pid
        || current.stopped.is_some()
        || now.seconds_since(current.at) < HEALTHY_AFTER_S
    {
        return false;
    }
    starts.len() >= CLEAN_STARTS
        && starts
            .iter()
            .rev()
            .take(CLEAN_STARTS)
            .enumerate()
            .all(|(index, start)| {
                !start.hung
                    && start.at > fault.failed_at
                    && (index == 0 || start.stopped.is_some())
                    && Commit::new(&start.commit).is_ok_and(|commit| {
                        commit != fault.commit
                            && git.is_ancestor(&fault.commit, &commit).unwrap_or(false)
                    })
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::fakes::{FakeClock, FakeGit, FakeHub};
    use ignite_core::traits::Clock;

    #[test]
    fn a_rejected_release_needs_three_consecutive_clean_starts_on_descendants() {
        let home = ignite_core::test_files::temp_dir().join("release-recovery");
        let clock = FakeClock::new(Timestamp::EPOCH);
        let git = FakeGit::default();
        let hub = FakeHub::default();
        let rejected = Commit::new("abc1234").unwrap();
        let rollback = Commit::new("deadbee").unwrap();
        let newer = Commit::new("cafecab").unwrap();
        git.set_not_ancestor(&rejected, &rollback);
        let report = JusticiarReport {
            subject: format!("release-selftest:{rejected}"),
            title: "Selftest failed".to_owned(),
            details: String::new(),
            evidence: format!("{rejected}@machine"),
            machine: "m-1".parse().unwrap(),
            host: "machine".to_owned(),
            on_machine: true,
        };
        queue_selftest(&home, &rejected, clock.now(), report.clone()).unwrap();
        hub.set_unreachable(true);
        send_pending(&home, &hub);
        hub.set_unreachable(false);
        send_pending(&home, &hub);
        send_pending(&home, &hub);
        assert_eq!(hub.justiciar_reports(), std::slice::from_ref(&report));

        let log = start_log(&Home { root: home.clone() });
        for (pid, commit, clean) in [
            (1, &newer, true),
            (2, &rollback, true),
            (3, &newer, false),
            (4, &newer, true),
            (5, &newer, true),
        ] {
            clock.advance(1.0);
            let started = clock.now();
            log.record(started, commit, pid).unwrap();
            clock.advance(HEALTHY_AFTER_S);
            send_cleared(&home, &git, &hub, clock.now(), pid);
            assert!(hub.justiciar_recoveries().is_empty());
            if clean {
                log.mark_stopped(pid, started, clock.now(), "clean")
                    .unwrap();
            }
        }
        clock.advance(1.0);
        log.record(clock.now(), &newer, 6).unwrap();
        send_cleared(&home, &git, &hub, clock.now(), 6);
        assert!(hub.justiciar_recoveries().is_empty());

        clock.advance(HEALTHY_AFTER_S);
        hub.set_unreachable(true);
        send_cleared(&home, &git, &hub, clock.now(), 6);
        hub.set_unreachable(false);
        send_cleared(&home, &git, &hub, clock.now(), 6);
        send_cleared(&home, &git, &hub, clock.now(), 6);
        assert_eq!(
            hub.justiciar_recoveries(),
            [JusticiarCleared {
                subject: report.subject,
                recovery: JusticiarRecovery::Sighting {
                    evidence: report.evidence
                },
            }]
        );
    }
}
