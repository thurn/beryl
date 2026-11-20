//! The supervisor's result file: writing it with retries, and telling a run whose result
//! could not be written from one whose supervisor was killed.

use super::*;

fn result() -> ResultFile {
    ResultFile {
        exit: Some(0),
        output: PathBuf::from("output.log"),
        usage: Usage::default(),
        hung: false,
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-result-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_result_is_written_whole_and_a_failed_write_is_an_error_after_its_tries() {
    let dir = scratch("write");
    write_result(&dir, &result(), Duration::ZERO).unwrap();
    let read: ResultFile = serde_json::from_slice(&fs::read(dir.join(RESULT)).unwrap()).unwrap();
    assert_eq!(read, result());
    fs::remove_dir_all(&dir).unwrap();
    // The folder is gone, as if the disk could not be written to.
    assert!(write_result(&dir, &result(), Duration::ZERO).is_err());
}

#[test]
fn a_run_whose_supervisor_could_not_write_the_result_is_told_apart_from_a_killed_one() {
    let dir = scratch("reader");
    fs::write(dir.join(SPEC), "{}").unwrap();
    fs::write(dir.join(OUTPUT), "the command's output\n").unwrap();
    fs::write(
        dir.join(SUPERVISOR_LOG),
        format!("ignite supervise: {RESULT_NOT_WRITTEN}: no space left\n"),
    )
    .unwrap();
    let run = Run { dir: dir.clone() };
    let lost = run.wait();
    assert_eq!((lost.exit, lost.output), (None, dir.join(OUTPUT)));

    // A supervisor that left nothing in its log was killed.
    fs::remove_file(dir.join(SUPERVISOR_LOG)).unwrap();
    let killed = run.wait();
    assert_eq!(
        (killed.exit, killed.output),
        (None, dir.join(SUPERVISOR_LOG))
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_command_that_exits_with_its_own_code_after_being_ended_was_ended_from_outside() {
    let dir = scratch("ended");
    let output = dir.join(OUTPUT);
    // `uv run` catches the scheduler's `SIGTERM` and exits 143 instead of dying from it.
    let caught = ResultFile {
        exit: Some(143),
        output: output.clone(),
        ..result()
    };
    write_result(&dir, &caught, Duration::ZERO).unwrap();
    assert!(!ResultFile::ended_from_outside(&output), "a plain failure");

    // `Run::end` marks the run before it signals the command. This group ID names no process.
    fs::write(dir.join(GROUP), "999999999").unwrap();
    let _ = Run { dir: dir.clone() }.end();
    assert!(ResultFile::ended_from_outside(&output));

    // A command the supervisor ended at its hang limit is still a failure.
    let hung = ResultFile {
        hung: true,
        ..caught
    };
    write_result(&dir, &hung, Duration::ZERO).unwrap();
    assert!(!ResultFile::ended_from_outside(&output));
    fs::remove_dir_all(&dir).unwrap();
}
