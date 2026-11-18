use std::cell::RefCell;
#[cfg(unix)]
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::fakes::FakeGit;

use super::*;

pub(super) fn tempdir(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "ignite-update-{}-{}-{name}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(super) fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

/// Writes a shell script, as stand-in for a built `ignite`. It is executable only on macOS,
/// where one test runs it.
pub(super) fn script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

pub(super) struct FakeBuilder {
    pub(super) out: PathBuf,
    pub(super) built: RefCell<Vec<Commit>>,
}

impl Builder for FakeBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        self.built.borrow_mut().push(commit.clone());
        script(&self.out, &format!("# built from {commit}\nexit 0"));
        Ok(self.out.clone())
    }
}

#[cfg(unix)]
#[derive(Default)]
struct FakeRestart {
    started: RefCell<Vec<(PathBuf, Vec<String>)>>,
}

#[cfg(unix)]
impl Restart for FakeRestart {
    fn restart(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        self.started
            .borrow_mut()
            .push((executable.to_owned(), args.to_vec()));
        Ok(())
    }
}

#[cfg(unix)]
#[test]
fn an_update_leaves_running_supervisors_on_the_old_executable() {
    let home = tempdir("update");
    let store = ExecutableStore::new(home.join("bin"));
    let old = commit("aaaaaaa");
    let new = commit("bbbbbbb");

    // A supervisor started from the old executable is still running a command.
    let old_exe = store.path(&old);
    script(&old_exe, "sleep 30");
    store.point_current_at(&old).unwrap();
    let before = std::fs::read(&old_exe).unwrap();
    let mut supervisor = Command::new(&old_exe)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let git = FakeGit::default();
    git.set_tip("master", new.clone());
    git.set_changed(
        &old,
        &new,
        &["ignite/crates/ignite_daemon/src/update.rs", "rust/x.rs"],
    );
    let builder = FakeBuilder {
        out: home.join("build").join("ignite"),
        built: RefCell::default(),
    };
    let updater = Updater {
        git: &git,
        builder: &builder,
        store: &store,
        master: "master",
        source: "ignite",
    };
    let UpdateCheck::Ready {
        from,
        to,
        executable,
    } = updater.check(&old).unwrap()
    else {
        panic!("an ignite change must produce a new executable");
    };
    let restart = FakeRestart::default();
    let error = restart_into(&restart, &executable);

    assert_eq!((from, to.clone()), (old.clone(), new.clone()));
    assert_eq!(executable, store.path(&new));
    assert_eq!(*builder.built.borrow(), std::slice::from_ref(&new));
    // The new daemon is started from the new executable, which is what the service links to.
    assert_eq!(
        *restart.started.borrow(),
        [(store.path(&new), vec!["daemon".to_owned()])]
    );
    assert_eq!(error.to_string(), "restarted");
    assert_eq!(
        ExecutableStore::version_of(&store.current_link()),
        Some(new)
    );
    // The old executable is byte for byte what it was, and its supervisor still runs.
    assert_eq!(std::fs::read(&old_exe).unwrap(), before);
    assert!(
        supervisor.try_wait().unwrap().is_none(),
        "the supervisor was stopped"
    );
    supervisor.kill().unwrap();
    supervisor.wait().unwrap();
}

#[test]
fn a_release_without_ignite_changes_builds_nothing() {
    let home = tempdir("unchanged");
    let store = ExecutableStore::new(home.join("bin"));
    let (old, new) = (commit("aaaaaaa"), commit("bbbbbbb"));
    let git = FakeGit::default();
    git.set_tip("master", new.clone());
    git.set_changed(&old, &new, &["rust/x.rs", "ignitefile.txt"]);
    let builder = FakeBuilder {
        out: home.join("build"),
        built: RefCell::default(),
    };
    let updater = Updater {
        git: &git,
        builder: &builder,
        store: &store,
        master: "master",
        source: "ignite",
    };
    assert_eq!(
        updater.check(&old).unwrap(),
        UpdateCheck::NotNeeded { tip: new }
    );
    assert_eq!(
        updater.check(&commit("bbbbbbb")).unwrap(),
        UpdateCheck::UpToDate
    );
    assert!(builder.built.borrow().is_empty());
}

#[test]
fn storing_a_commit_twice_keeps_the_first_file() {
    let home = tempdir("store");
    let store = ExecutableStore::new(home.join("bin"));
    let c = commit("ccccccc");
    script(&home.join("one"), "echo one");
    script(&home.join("two"), "echo two");
    let path = store.store(&c, &home.join("one")).unwrap();
    let again = store.store(&c, &home.join("two")).unwrap();
    assert_eq!(path, again);
    assert!(std::fs::read_to_string(path).unwrap().contains("echo one"));
}

#[test]
fn a_daemon_on_another_version_drains_then_updates() {
    let own = commit("aaaaaaa");
    assert_eq!(version_action(&own, "aaaaaaa", 3), VersionAction::Current);
    assert_eq!(version_action(&own, "bbbbbbb", 3), VersionAction::Drain);
    assert_eq!(version_action(&own, "bbbbbbb", 0), VersionAction::UpdateNow);
    assert!(VersionAction::Current.may_ask_for_work());
    assert!(!VersionAction::Drain.may_ask_for_work());
    assert!(!VersionAction::UpdateNow.may_ask_for_work());
}

#[test]
fn a_release_tip_that_does_not_contain_the_running_commit_is_not_an_update() {
    let home = tempdir("ahead");
    let store = ExecutableStore::new(home.join("bin"));
    let (running, tip) = (commit("ccccccc"), commit("ddddddd"));
    let git = FakeGit::default();
    git.set_tip("master", tip.clone());
    git.set_changed(
        &running,
        &tip,
        &["ignite/crates/ignite_daemon/src/update.rs"],
    );
    git.set_not_ancestor(&running, &tip);
    let builder = FakeBuilder {
        out: home.join("build"),
        built: RefCell::default(),
    };
    let updater = Updater {
        git: &git,
        builder: &builder,
        store: &store,
        master: "master",
        source: "ignite",
    };
    assert_eq!(updater.check(&running).unwrap(), UpdateCheck::Ahead { tip });
    assert!(builder.built.borrow().is_empty());
}
