//! The recordings pack: built once for each state of the sources, whole however many processes
//! ask for it at once, and named by a hash that follows the sources' contents. A stand-in
//! recorder takes the place of the engine, so no test here plays a game.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use beryl_inspect::pack::{self, Recorded, Recorder, sources};
use beryl_protocol::Seat;
use beryl_protocol::ipc::FixtureMessage;

/// A recorder that counts its calls and makes two small recordings and one failure.
#[derive(Default)]
struct Counting {
    calls: AtomicUsize,
    /// A file to add a line to on each call, for a count across processes.
    log: Option<PathBuf>,
}

impl Recorder for Counting {
    fn record(&self) -> Vec<Recorded> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(log) = &self.log {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
                .unwrap();
            writeln!(file, "{}", std::process::id()).unwrap();
        }
        recordings()
    }
}

fn recordings() -> Vec<Recorded> {
    let message = |n: u32| FixtureMessage::Event {
        event: "game-update".to_owned(),
        payload: serde_json::json!({ "n": n }),
    };
    vec![
        Recorded {
            name: "verbs-zones".to_owned(),
            seat: Seat::P1,
            messages: Ok(vec![message(1), message(2)]),
        },
        Recorded {
            name: "counterspell/cast".to_owned(),
            seat: Seat::P0,
            messages: Ok(vec![FixtureMessage::response("choose", &()), message(3)]),
        },
        Recorded {
            name: "broken".to_owned(),
            seat: Seat::P0,
            messages: Err("the game ended before its stop".to_owned()),
        },
    ]
}

/// An empty directory of its own for `test`, under the test target's scratch directory.
fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("pack-{test}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn files(dir: &Path, suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(suffix))
        .collect();
    names.sort();
    names
}

#[test]
fn a_second_call_with_unchanged_sources_records_nothing() {
    let dir = scratch("second-call");
    let recorder = Counting::default();
    let first = pack::ensure(&dir, "0123456789abcdef", &recorder).unwrap();
    let second = pack::ensure(&dir, "0123456789abcdef", &recorder).unwrap();
    assert_eq!(first, second);
    assert_eq!(recorder.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn a_pack_reads_back_each_recording_sorted_with_its_views_and_failures() {
    let dir = scratch("read-back");
    let path = pack::ensure(&dir, "feed", &Counting::default()).unwrap();
    let bytes = std::fs::read(path).unwrap();
    let entries = pack::read(&bytes).unwrap();
    let names: Vec<(&str, Seat, u32)> = entries
        .iter()
        .map(|e| (e.name, e.seat(), e.views))
        .collect();
    assert_eq!(
        names,
        [
            ("broken", Seat::P0, 0),
            ("counterspell/cast", Seat::P0, 1),
            ("verbs-zones", Seat::P1, 2),
        ]
    );
    let made = recordings();
    assert_eq!(
        pack::messages(&entries[2]).unwrap(),
        *made[0].messages.as_ref().unwrap()
    );
    assert_eq!(
        pack::messages(&entries[0]).unwrap_err(),
        "the game ended before its stop"
    );
}

/// Sets the modification time of `dir`'s pack for `hash` to `hours` ago.
fn age(dir: &Path, hash: &str, hours: u64) {
    let file = std::fs::File::options()
        .write(true)
        .open(pack::path(dir, hash))
        .unwrap();
    let then = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600);
    file.set_modified(then).unwrap();
}

#[test]
fn packs_used_lately_are_kept_and_old_ones_are_deleted() {
    let dir = scratch("kept");
    let recorder = Counting::default();
    for hash in ["01", "02", "03", "04", "05"] {
        pack::ensure(&dir, hash, &recorder).unwrap();
    }
    // Five packs built within the last hours stay, more than the three newest.
    assert_eq!(files(&dir, ".bin").len(), 5);
    for hash in ["01", "02", "03"] {
        age(&dir, hash, 7);
    }
    // A worktree still reading pack 02 keeps it.
    pack::mark_used(&pack::path(&dir, "02"));
    pack::ensure(&dir, "06", &recorder).unwrap();
    assert_eq!(
        files(&dir, ".bin"),
        [
            "recordings-02.bin",
            "recordings-04.bin",
            "recordings-05.bin",
            "recordings-06.bin"
        ]
    );
    assert_eq!(files(&dir, ".lock").len(), 4);
}

#[test]
fn the_newest_three_packs_are_kept_however_old() {
    let dir = scratch("kept-old");
    let recorder = Counting::default();
    for hash in ["01", "02", "03"] {
        pack::ensure(&dir, hash, &recorder).unwrap();
        age(&dir, hash, 24);
    }
    pack::ensure(&dir, "04", &recorder).unwrap();
    assert_eq!(files(&dir, ".bin").len(), 3);
}

/// The environment variable that makes [`build_in_a_child_process`] build: the directory.
const CHILD_DIR: &str = "BERYL_INSPECT_PACK_CHILD_DIR";

/// Run by [`two_processes_at_once_leave_one_whole_file`] in each of its children.
#[test]
#[ignore = "a child process of two_processes_at_once_leave_one_whole_file"]
fn build_in_a_child_process() {
    let dir = PathBuf::from(std::env::var_os(CHILD_DIR).expect("run by its parent test"));
    let recorder = Counting {
        log: Some(dir.join("calls.log")),
        ..Counting::default()
    };
    let path = pack::ensure(&dir, "cafe", &recorder).unwrap();
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(pack::read(&bytes).unwrap().len(), 3);
}

#[test]
fn two_processes_at_once_leave_one_whole_file() {
    let dir = scratch("two-processes");
    // The harness names a test by its module path without the crate's name.
    let module = module_path!().split_once("::").map_or("", |(_, path)| path);
    let name = format!("{module}::build_in_a_child_process");
    let child = || {
        Command::new(std::env::current_exe().unwrap())
            .args([name.as_str(), "--exact", "--ignored", "--quiet"])
            .env(CHILD_DIR, &dir)
            .spawn()
            .unwrap()
    };
    let children = [child(), child()];
    for child in children {
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "a child failed");
    }
    assert_eq!(files(&dir, ".bin"), ["recordings-cafe.bin"]);
    assert!(files(&dir, ".tmp").is_empty());
    let calls = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert_eq!(calls.lines().count(), 1, "one process built the pack");
    let bytes = std::fs::read(dir.join("recordings-cafe.bin")).unwrap();
    assert_eq!(pack::read(&bytes).unwrap().len(), 3);
}

/// The repository this test was built from.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn the_sources_are_the_recorder_and_its_data_and_not_the_client() {
    let files = sources::sources(&root()).unwrap();
    let has = |path: &str| files.iter().any(|f| f == Path::new(path));
    assert!(has("ui/fixtures.json"));
    assert!(has("data/card-store.tsv"));
    assert!(has("rust/crates/beryl_cli/src/fixture.rs"));
    assert!(has("rust/crates/beryl_engine/Cargo.toml"));
    assert!(has("rust/crates/beryl_session/src/lib.rs"));
    assert!(files.iter().any(|f| f.starts_with("rust/cards")));
    assert!(files.iter().any(|f| f.starts_with("docs/sample-decks")));
    assert!(!files.iter().any(|f| f.starts_with("rust/beryl3d")));
}

#[test]
fn a_changed_byte_in_the_fixture_definitions_changes_the_hash() {
    let dir = scratch("hash");
    std::fs::create_dir_all(dir.join("ui")).unwrap();
    let mut bytes = std::fs::read(root().join("ui/fixtures.json")).unwrap();
    let file = dir.join("ui/fixtures.json");
    std::fs::write(&file, &bytes).unwrap();
    let listed = [PathBuf::from("ui/fixtures.json")];
    let before = sources::hash(&dir, &listed).unwrap();
    assert_eq!(before, sources::hash(&dir, &listed).unwrap());
    let middle = bytes.len() / 2;
    bytes[middle] ^= 1;
    std::fs::write(&file, &bytes).unwrap();
    assert_ne!(before, sources::hash(&dir, &listed).unwrap());
}
