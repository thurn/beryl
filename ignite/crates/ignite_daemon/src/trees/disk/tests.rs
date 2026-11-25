//! The sweep of replaced builds over Cargo profile directories written by hand: fingerprint
//! folders whose JSON records each unit's settings and its dependencies' fingerprints.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use super::sweep_superseded;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A target directory under the system temp directory, deleted when dropped.
struct Target(PathBuf);

impl Target {
    fn new() -> Target {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-sweep-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("debug/.fingerprint")).unwrap();
        Target(dir)
    }

    fn profile(&self) -> PathBuf {
        self.0.join("debug")
    }

    /// Writes unit `name` (`<crate>-<hash>`) as [`Target::unit_at`] does, written at 1000 s.
    fn unit(&self, name: &str, kind: &str, fingerprint: u64, settings: &str, deps: &[(&str, u64)]) {
        self.unit_at(name, kind, fingerprint, settings, deps, 1000);
    }

    /// Writes unit `name` (`<crate>-<hash>`) with fingerprint file `kind`, fingerprint
    /// `fingerprint`, the JSON `settings` and `deps` as `(name, fingerprint)`, and a library in
    /// `deps/`, all last written `age` seconds after the epoch.
    fn unit_at(
        &self,
        name: &str,
        kind: &str,
        fingerprint: u64,
        settings: &str,
        deps: &[(&str, u64)],
        age: u64,
    ) {
        let folder = self.profile().join(".fingerprint").join(name);
        fs::create_dir_all(&folder).unwrap();
        let deps: Vec<String> = deps
            .iter()
            .map(|(dep, f)| format!("[1,\"{dep}\",false,{f}]"))
            .collect();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(age);
        let files = [
            (
                folder.join(kind),
                format!("{:016x}", fingerprint.swap_bytes()),
            ),
            (
                folder.join(format!("{kind}.json")),
                format!("{{{settings}\"deps\":[{}]}}", deps.join(",")),
            ),
            (
                self.profile().join("deps").join(format!("lib{name}.rlib")),
                String::new(),
            ),
        ];
        for (path, text) in files {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, text).unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(at)
                .unwrap();
        }
    }

    fn sweep(&self) -> Vec<String> {
        sweep_superseded(&self.0)
            .iter()
            .map(|folder| name_of(folder))
            .collect()
    }

    fn has(&self, name: &str) -> bool {
        self.profile().join(".fingerprint").join(name).exists()
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn name_of(folder: &Path) -> String {
    folder.file_name().unwrap().to_string_lossy().into_owned()
}

const DEV: &str = r#""profile":1,"features":"[]","#;
const TEST: &str = r#""profile":2,"features":"[]","#;
/// What Cargo records for a unit that runs a build script.
const RUN: &str = r#""profile":0,"features":"","target":0,"#;

#[test]
fn build_script_runs_of_two_profiles_are_both_kept() {
    let t = Target::new();
    // Each profile builds the library against its own run of the build script, and Cargo
    // records both runs with the same zeroed settings.
    t.unit(
        "pm-aaaa",
        "run-build-script-build-script-build",
        11,
        RUN,
        &[("build_script_build", 1)],
    );
    t.unit(
        "pm-bbbb",
        "run-build-script-build-script-build",
        12,
        RUN,
        &[("build_script_build", 2)],
    );
    t.unit("pm-cccc", "build-script-build-script-build", 1, DEV, &[]);
    t.unit("pm-dddd", "build-script-build-script-build", 2, TEST, &[]);
    t.unit_at(
        "pm-eeee",
        "lib-pm",
        21,
        DEV,
        &[("build_script_build", 11)],
        900,
    );
    t.unit_at(
        "pm-ffff",
        "lib-pm",
        22,
        TEST,
        &[("build_script_build", 12)],
        1100,
    );
    assert_eq!(t.sweep(), Vec::<String>::new());
}

#[test]
fn a_library_two_live_builds_use_with_other_dependencies_is_kept() {
    let t = Target::new();
    // Two programs of different crates, each against its own build of `shared`, whose
    // settings are the same and whose dependencies differ.
    t.unit("dep-aaaa", "lib-dep", 1, DEV, &[]);
    t.unit(
        "dep-bbbb",
        "lib-dep",
        2,
        r#""profile":1,"features":"[\"x\"]","#,
        &[],
    );
    t.unit_at("shared-aaaa", "lib-shared", 11, DEV, &[("dep", 1)], 900);
    t.unit_at("shared-bbbb", "lib-shared", 12, DEV, &[("dep", 2)], 1100);
    t.unit("one-aaaa", "bin-one", 21, DEV, &[("shared", 11)]);
    t.unit("two-aaaa", "bin-two", 22, DEV, &[("shared", 12)]);
    assert_eq!(t.sweep(), Vec::<String>::new());
}

#[test]
fn a_replaced_program_goes_with_the_dependencies_only_it_used() {
    let t = Target::new();
    t.unit("dep-aaaa", "lib-dep", 1, DEV, &[]);
    t.unit_at("core-aaaa", "lib-core", 11, DEV, &[("dep", 1)], 500);
    t.unit_at("core-bbbb", "lib-core", 12, DEV, &[("dep", 1)], 1500);
    t.unit_at("app-aaaa", "bin-app", 21, DEV, &[("core", 11)], 600);
    t.unit_at("app-bbbb", "bin-app", 22, DEV, &[("core", 12)], 1600);
    assert_eq!(t.sweep(), ["app-aaaa", "core-aaaa"]);
    assert!(!t.profile().join("deps/libcore-aaaa.rlib").exists());
    assert!(!t.has("core-aaaa") && !t.has("app-aaaa"));
    assert!(t.has("dep-aaaa") && t.has("core-bbbb") && t.has("app-bbbb"));
}

#[test]
fn every_build_of_a_dependency_rebuilt_in_place_is_kept() {
    let t = Target::new();
    // `app` recorded a fingerprint of `core` that no unit has any more: `core` was rebuilt in
    // its own folder since. Which build `app` will use is unknown, so both are kept.
    t.unit_at("core-aaaa", "lib-core", 13, DEV, &[], 500);
    t.unit_at("core-bbbb", "lib-core", 14, DEV, &[], 1500);
    t.unit("app-aaaa", "bin-app", 21, DEV, &[("core", 12)]);
    assert_eq!(t.sweep(), Vec::<String>::new());
}

#[test]
fn a_fingerprint_folder_that_cannot_be_deleted_keeps_its_outputs() {
    let t = Target::new();
    t.unit_at("app-aaaa", "bin-app", 21, DEV, &[], 600);
    t.unit_at("app-bbbb", "bin-app", 22, DEV, &[], 1600);
    // A folder inside the replaced unit's fingerprint folder that nothing can be deleted from
    // stands for a sweep that stops partway through that folder.
    let stuck = t.profile().join(".fingerprint/app-aaaa/stuck");
    fs::create_dir(&stuck).unwrap();
    fs::write(stuck.join("file"), "").unwrap();
    let blocked = crate::test_process::block_deletion(&stuck.join("file"));
    let swept = t.sweep();
    drop(blocked);
    // Cargo reads a surviving fingerprint, so the files it names must survive with it.
    assert_eq!(swept, Vec::<String>::new());
    assert!(t.profile().join("deps/libapp-aaaa.rlib").exists());
}

#[test]
fn ceiling_cleanup_stops_after_the_first_stage_that_makes_room() {
    let write_bytes = |file: &Path, count| {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, vec![b'x'; count]).unwrap();
    };
    for (ceiling, expected) in [
        (770, vec!["replaced objects and builds"]),
        (
            650,
            vec![
                "replaced objects and builds",
                "unused Cargo fingerprints and outputs",
            ],
        ),
        (
            520,
            vec![
                "replaced objects and builds",
                "unused Cargo fingerprints and outputs",
                "incremental caches",
            ],
        ),
        (
            420,
            vec![
                "replaced objects and builds",
                "unused Cargo fingerprints and outputs",
                "incremental caches",
                "build directories",
            ],
        ),
    ] {
        let target = Target::new();
        let profile = target.profile();
        // Different features keep these both in the replaced-unit pass. The previous build
        // used only the second, as shown by its Cargo fingerprint trace.
        let current = profile.join(".fingerprint/pkg-bbbb/lib-pkg");
        let unused = profile.join(".fingerprint/pkg-aaaa/lib-pkg");
        for (fingerprint, unit, age) in [(&current, "bbbb", 2), (&unused, "aaaa", 1)] {
            write_bytes(fingerprint, 10);
            write_bytes(&profile.join(format!("deps/libpkg-{unit}.rlib")), 100);
            let object = profile.join(format!("deps/pkg-bbbb.cgu.abc{age}.rcgu.o"));
            write_bytes(&object, if age == 1 { 240 } else { 120 });
            fs::File::options()
                .write(true)
                .open(&object)
                .unwrap()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(age))
                .unwrap();
        }
        // Keep the units distinct in the replaced-unit pass.
        fs::write(unused.with_extension("json"), "{\"features\":\"legacy\"}").unwrap();
        write_bytes(&profile.join("incremental/cache"), 100);
        write_bytes(&target.0.join("other-output"), 130);
        let log = target.0.with_extension("log");
        let canonical = ignite_os::paths::canonicalize(&current).unwrap();
        fs::write(
            &log,
            format!(
                "DEBUG cargo::core::compiler::fingerprint: fingerprint at: {}\n",
                canonical.display()
            ),
        )
        .unwrap();
        let build = super::Measurement::read(std::slice::from_ref(&target.0), Some(&log));
        fs::remove_file(log).unwrap();
        assert!(build.bytes.unwrap() > ceiling);
        let mut stages = Vec::new();
        super::clean(
            std::slice::from_ref(&target.0),
            ceiling,
            &build.fingerprints,
            |stage| {
                stages.push(stage.to_owned());
            },
        )
        .unwrap();
        assert_eq!(stages, expected);
        assert_eq!(current.is_file(), stages.len() < 4);
        assert_eq!(unused.is_file(), stages.len() < 2);
        assert_eq!(profile.join("incremental/cache").exists(), stages.len() < 3);
        assert_eq!(target.0.exists(), stages.len() < 4);
    }
}

#[test]
fn ceiling_cleanup_keeps_the_incremental_caches_compiled_last() {
    let target = Target::new();
    let incremental = target.profile().join("incremental");
    fs::create_dir_all(target.profile().join(".fingerprint")).unwrap();
    // Three crates' caches of 100 bytes, compiled at times 1, 2 and 3.
    for (name, age) in [("old-aaaa", 1), ("middle-bbbb", 2), ("new-cccc", 3)] {
        let session = incremental.join(name).join("s-session");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("query-cache.bin"), vec![b'x'; 100]).unwrap();
        for dir in [session, incremental.join(name)] {
            fs::File::open(&dir)
                .unwrap()
                .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(age))
                .unwrap();
        }
    }
    // 300 bytes against a ceiling of 260 and a target of 208: one cache must go.
    let mut stages = Vec::new();
    super::clean(
        std::slice::from_ref(&target.0),
        260,
        &std::collections::BTreeSet::new(),
        |stage| stages.push(stage.to_owned()),
    )
    .unwrap();
    assert_eq!(stages, ["incremental caches"]);
    assert!(!incremental.join("old-aaaa").exists());
    assert!(incremental.join("middle-bbbb").exists());
    assert!(incremental.join("new-cccc").exists());
}
