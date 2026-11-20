use std::path::PathBuf;

use ignite_core::ids::CheckName;
use ignite_core::protocol::Usage;

use super::*;

fn reported(check: &str, outcome: Outcome) -> Reported {
    Reported {
        check: CheckName::new(check).unwrap(),
        outcome,
        at: None,
        message: None,
        reason: None,
    }
}

fn ended(exit: Option<i32>, hung: bool) -> RunResult {
    RunResult {
        exit,
        output: PathBuf::from("/nonexistent/output.log"),
        usage: Usage::default(),
        hung,
    }
}

#[test]
fn a_run_is_unstarted_when_three_or_more_checks_all_failed_or_nothing_reported() {
    let failed: Vec<Reported> = ["unit:a", "unit:b", "unit:c"]
        .iter()
        .map(|c| reported(c, Outcome::Fail))
        .collect();
    assert!(looks_unstarted(&failed, &ended(Some(100), false), 3));
    assert!(
        !looks_unstarted(&failed[..2], &ended(Some(100), false), 2),
        "two failures can be real failures of two checks"
    );
    let mut one_passed = failed.clone();
    one_passed.push(reported("unit:d", Outcome::Pass));
    assert!(!looks_unstarted(&one_passed, &ended(Some(100), false), 4));
    assert!(looks_unstarted(&[], &ended(Some(127), false), 1));
    assert!(
        !looks_unstarted(&[], &ended(None, true), 1),
        "a hang is not a start failure"
    );
    assert!(!looks_unstarted(&[], &ended(Some(0), false), 1));
}

#[test]
fn the_description_quotes_the_line_every_failure_repeated_and_the_machine() {
    let mut first = reported("unit:a", Outcome::Fail);
    first.message = Some("exit code 126".to_owned());
    let reports = [
        first,
        reported("unit:b", Outcome::Fail),
        reported("unit:c", Outcome::Fail),
    ];
    let missing = "runner: line 2: /bin/1234567/ignite: No such file or directory";
    let output = format!(
        "{{\"type\":\"test\"}}\n{{\"type\":\"test\"}}\n{{\"type\":\"test\"}}\n\
         ────────\n────────\n────────\n\
         FAIL a\n{missing}\nFAIL b\n{missing}\nFAIL c\n{missing}\n"
    );
    let machine = Machine {
        open_files: Some(12),
        processes: Some((2000, 1500)),
        process_limit: Some("2666".to_owned()),
        disk_free_gb: Some(169),
        ..Machine::default()
    };
    let text = describe(&reports, &output, &machine);
    assert!(text.contains("all 3 checks in this run failed"), "{text}");
    assert!(
        text.contains("first failure: unit:a: exit code 126"),
        "{text}"
    );
    assert!(
        text.contains(&format!("repeated in every failure: 3x {missing}")),
        "{text}"
    );
    assert!(!text.contains("3x {"), "events are not quoted: {text}");
    assert!(!text.contains("3x ───"), "decoration is not quoted: {text}");
    assert!(text.contains("processes of this user: 2000, 1500 of them zombies (limit `ulimit -u` 2666, kern.maxprocperuid unknown)"), "{text}");
    assert!(
        text.contains("open files of the daemon: 12 (limit per process unknown)"),
        "{text}"
    );
    assert!(
        text.contains("free disk where the tree is: 169 GB"),
        "{text}"
    );
}

#[test]
fn free_memory_counts_free_and_speculative_pages() {
    let vm_stat = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
                   Pages free:                               64000.\n\
                   Pages active:                            900000.\n\
                   Pages speculative:                        64000.\n";
    assert_eq!(free_mb(vm_stat), Some(2000));
    assert_eq!(free_mb("nothing"), None);
}

#[test]
fn reading_the_machine_measures_this_process() {
    let machine = Machine::read(&ignite_core::test_files::temp_dir());
    if cfg!(target_os = "macos") {
        assert!(machine.open_files.is_some_and(|n| n > 0));
        assert!(machine.processes.is_some_and(|(all, _)| all > 0));
        assert!(machine.free_memory_mb.is_some());
        assert!(machine.disk_free_gb.is_some());
    }
}
