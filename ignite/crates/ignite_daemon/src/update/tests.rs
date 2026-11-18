use std::cell::RefCell;
#[cfg(unix)]
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};

use ignite_core::fakes::FakeGit;

use super::*;

fn tempdir(name: &str) -> PathBuf {
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

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

/// Writes a shell script, as stand-in for a built `ignite`. It is executable only on macOS,
/// where one test runs it.
fn script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

struct FakeBuilder {
    out: PathBuf,
    built: RefCell<Vec<Commit>>,
}

impl Builder for FakeBuilder {
    fn build(&self, commit: &Commit) -> Result<PathBuf> {
        self.built.borrow_mut().push(commit.clone());
        script(&self.out, &format!("# built from {commit}\nexit 0"));
        Ok(self.out.clone())
    }
}

#[derive(Default)]
struct FakeService {
    installed: RefCell<Vec<(PathBuf, Vec<String>)>>,
}

impl Service for FakeService {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        self.installed
            .borrow_mut()
            .push((executable.to_owned(), args.to_vec()));
        Ok(())
    }
    fn uninstall(&self) -> io::Result<()> {
        Ok(())
    }
    fn is_installed(&self) -> io::Result<bool> {
        Ok(!self.installed.borrow().is_empty())
    }
}

struct FakeRegistrar {
    requests: RefCell<Vec<MachineRegister>>,
    down: bool,
}

impl Registrar for FakeRegistrar {
    fn register(&self, request: &MachineRegister) -> Result<MachineRegistered> {
        if self.down {
            return Err(Error::Unreachable("hub is down".to_owned()));
        }
        self.requests.borrow_mut().push(request.clone());
        Ok(MachineRegistered {
            machine: request.machine.clone(),
            token: "write-token".to_owned(),
        })
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

struct FakeHost;

impl Host for FakeHost {
    fn hostname(&self) -> String {
        "testbox".to_owned()
    }
    fn cores(&self) -> u32 {
        8
    }
    fn memory_mb(&self) -> u64 {
        16384
    }
    fn disk_free_gb(&self, _path: &Path) -> Option<u64> {
        Some(500)
    }
    fn tool_path(&self, _name: &str) -> Option<PathBuf> {
        None
    }
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

fn config() -> Config {
    Config::parse(
        "ignite.toml",
        r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.testbox]
        os = "macos"
        ticket_trees = 2
        landing_trees = 1
        families = ["rust"]
        "#,
    )
    .unwrap()
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
fn install_stores_the_build_registers_and_starts_the_service() {
    let home = tempdir("install");
    let store = ExecutableStore::new(home.join("bin"));
    let identity = IdentityFile {
        path: home.join("machine.json"),
    };
    let builder = FakeBuilder {
        out: home.join("build").join("ignite"),
        built: RefCell::default(),
    };
    let registrar = FakeRegistrar {
        requests: RefCell::default(),
        down: false,
    };
    let service = FakeService::default();
    let deps = InstallDeps {
        host: &FakeHost,
        builder: &builder,
        store: &store,
        identity: &identity,
        registrar: &registrar,
        service: &service,
    };
    let head = commit("1234abc");
    let done = install(&deps, &config(), &head).unwrap();

    assert!(store.has(&head));
    assert_eq!(done.executable, store.path(&head));
    let request = registrar.requests.borrow()[0].clone();
    assert_eq!(
        (
            request.name.as_str(),
            request.cores,
            request.version.as_str()
        ),
        ("testbox", 8, "1234abc")
    );
    assert_eq!(request.families, ["rust"]);
    assert_eq!(
        *service.installed.borrow(),
        [(store.current_link(), vec!["daemon".to_owned()])]
    );
    let saved = identity.load().unwrap().unwrap();
    assert_eq!(saved.token.as_deref(), Some("write-token"));
    assert_eq!(saved.machine, done.machine);
    // A second install keeps the machine's ID.
    let again = install(&deps, &config(), &head).unwrap();
    assert_eq!(again.machine, done.machine);
}

#[test]
fn install_stops_before_the_service_when_the_hub_is_down() {
    let home = tempdir("hubdown");
    let store = ExecutableStore::new(home.join("bin"));
    let identity = IdentityFile {
        path: home.join("machine.json"),
    };
    let builder = FakeBuilder {
        out: home.join("build"),
        built: RefCell::default(),
    };
    let registrar = FakeRegistrar {
        requests: RefCell::default(),
        down: true,
    };
    let service = FakeService::default();
    let deps = InstallDeps {
        host: &FakeHost,
        builder: &builder,
        store: &store,
        identity: &identity,
        registrar: &registrar,
        service: &service,
    };
    let error = install(&deps, &config(), &commit("1234abc")).unwrap_err();
    assert!(matches!(error, Error::Unreachable(_)));
    assert_eq!(failure(&error).exit, Exit::Unreachable);
    assert!(service.installed.borrow().is_empty());
    assert!(identity.load().unwrap().unwrap().token.is_none());
}

#[test]
fn install_refuses_a_machine_that_ignite_toml_does_not_declare() {
    struct Other;
    impl Host for Other {
        fn hostname(&self) -> String {
            "stranger".to_owned()
        }
        fn cores(&self) -> u32 {
            1
        }
        fn memory_mb(&self) -> u64 {
            1
        }
        fn disk_free_gb(&self, _: &Path) -> Option<u64> {
            None
        }
        fn tool_path(&self, _: &str) -> Option<PathBuf> {
            None
        }
        fn path_exists(&self, _: &Path) -> bool {
            false
        }
    }
    let home = tempdir("stranger");
    let store = ExecutableStore::new(home.join("bin"));
    let identity = IdentityFile {
        path: home.join("machine.json"),
    };
    let builder = FakeBuilder {
        out: home.join("build"),
        built: RefCell::default(),
    };
    let registrar = FakeRegistrar {
        requests: RefCell::default(),
        down: false,
    };
    let service = FakeService::default();
    let deps = InstallDeps {
        host: &Other,
        builder: &builder,
        store: &store,
        identity: &identity,
        registrar: &registrar,
        service: &service,
    };
    let error = install(&deps, &config(), &commit("1234abc")).unwrap_err();
    assert!(error.to_string().contains("[machine.stranger]"), "{error}");
    assert!(builder.built.borrow().is_empty());
}
