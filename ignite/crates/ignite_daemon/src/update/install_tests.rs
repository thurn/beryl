use std::cell::RefCell;

use ignite_core::config::Config;
use ignite_core::protocol::hub::{MachineRegister, MachineRegistered};

use super::super::tests::{FakeBuilder, commit, script, tempdir};
use super::*;

/// One hub service the install wrote.
#[derive(Debug, PartialEq, Eq)]
struct HubInstall {
    executable: PathBuf,
    args: Vec<String>,
    environment: Vec<(String, String)>,
    /// How many daemon services had been written before it.
    daemons_before: usize,
}

/// An install's environment variables.
type Environment = Box<dyn Fn(&str) -> Option<String>>;

#[derive(Default)]
struct FakeService {
    installed: RefCell<Vec<(PathBuf, Vec<String>)>>,
    monitors: RefCell<Vec<(PathBuf, Vec<String>)>>,
    hubs: RefCell<Vec<HubInstall>>,
    /// The variables the installed services already start with.
    recorded: Vec<(String, String)>,
    /// Fail every daemon install, as `launchctl bootstrap` does when launchd refuses the agent.
    refuse: bool,
}

impl Service for FakeService {
    fn install(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        if self.refuse {
            return Err(io::Error::other("launchctl bootstrap: Bootstrap failed: 5"));
        }
        self.installed
            .borrow_mut()
            .push((executable.to_owned(), args.to_vec()));
        Ok(())
    }
    fn install_monitor(&self, executable: &Path, args: &[String]) -> io::Result<()> {
        self.monitors
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
    fn install_hub(
        &self,
        executable: &Path,
        args: &[String],
        environment: &[(String, String)],
    ) -> io::Result<()> {
        self.hubs.borrow_mut().push(HubInstall {
            executable: executable.to_owned(),
            args: args.to_vec(),
            environment: environment.to_vec(),
            daemons_before: self.installed.borrow().len(),
        });
        Ok(())
    }
    fn recorded_variable(&self, name: &str) -> io::Result<Option<String>> {
        Ok(self
            .recorded
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone()))
    }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

struct FakeRegistrar {
    requests: RefCell<Vec<MachineRegister>>,
    /// The token each registration sent.
    tokens: RefCell<Vec<Option<String>>>,
    down: bool,
}

impl Registrar for FakeRegistrar {
    fn register(
        &self,
        request: &MachineRegister,
        token: Option<&str>,
    ) -> Result<MachineRegistered> {
        self.tokens.borrow_mut().push(token.map(str::to_owned));
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

/// Records the hand-offs it is asked for and answers each one as the new build would.
#[derive(Default)]
struct FakeHandoff {
    calls: RefCell<Vec<(PathBuf, Commit)>>,
}

impl Handoff for FakeHandoff {
    fn finish(&self, executable: &Path, built: &Commit) -> Reply {
        self.calls
            .borrow_mut()
            .push((executable.to_owned(), built.clone()));
        Reply::text(Exit::Success, "finished by the new build")
    }
}

/// One install's home folder and fakes.
struct Fixture {
    home: PathBuf,
    store: ExecutableStore,
    identity: IdentityFile,
    builder: FakeBuilder,
    registrar: FakeRegistrar,
    service: FakeService,
    handoff: FakeHandoff,
    /// The install's environment variables.
    environment: Environment,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let home = tempdir(name);
        Fixture {
            store: ExecutableStore::new(home.join("bin")),
            identity: IdentityFile {
                path: home.join("machine.json"),
            },
            builder: FakeBuilder {
                out: home.join("build").join("ignite"),
                built: RefCell::default(),
            },
            registrar: FakeRegistrar {
                requests: RefCell::default(),
                tokens: RefCell::default(),
                down: false,
            },
            service: FakeService::default(),
            handoff: FakeHandoff::default(),
            environment: Box::new(|_| None),
            home,
        }
    }

    /// Sets the install's environment variables.
    fn with_environment(mut self, list: &[(&str, &str)]) -> Fixture {
        let list = pairs(list);
        self.environment =
            Box::new(move |name| list.iter().find(|(n, _)| n == name).map(|(_, v)| v.clone()));
        self
    }

    /// The install's collaborators, run by `running`.
    fn deps<'a>(&'a self, host: &'a dyn Host, running: &'a Path) -> InstallDeps<'a> {
        InstallDeps {
            host,
            builder: &self.builder,
            store: &self.store,
            identity: &self.identity,
            registrar: &self.registrar,
            service: &self.service,
            running,
            handoff: &self.handoff,
            environment: self.environment.as_ref(),
        }
    }
}

fn installed(outcome: Outcome) -> Installed {
    match outcome {
        Outcome::Installed(done) => done,
        Outcome::HandedOff(reply) => panic!("handed off: {}", reply.text),
    }
}

#[test]
fn install_stores_the_build_registers_and_starts_the_service() {
    let fx = Fixture::new("install");
    let head = commit("1234abc");
    // The install runs in the build it makes, so it finishes in this process.
    let running = fx.store.path(&head);
    let deps = fx.deps(&FakeHost, &running);
    let done = installed(install(&deps, &config(), &head).unwrap());

    assert!(fx.store.has(&head));
    assert_eq!(done.executable, fx.store.path(&head));
    assert!(fx.handoff.calls.borrow().is_empty());
    let request = fx.registrar.requests.borrow()[0].clone();
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
        *fx.service.installed.borrow(),
        [(fx.store.current_link(), vec!["daemon".to_owned()])]
    );
    // No build has stayed up yet, so the monitor runs the current one.
    assert!(done.monitor);
    assert_eq!(
        *fx.service.monitors.borrow(),
        [(fx.store.current_link(), vec!["monitor".to_owned()])]
    );
    let saved = fx.identity.load().unwrap().unwrap();
    assert_eq!(saved.token.as_deref(), Some("write-token"));
    assert_eq!(saved.machine, done.machine);
    // A second install keeps the machine's ID.
    let again = installed(install(&deps, &config(), &head).unwrap());
    assert_eq!(again.machine, done.machine);
    // The first registration had no token to send; the second sends the one it was given.
    assert_eq!(
        *fx.registrar.tokens.borrow(),
        [None, Some("write-token".to_owned())]
    );
}

#[test]
fn an_install_on_another_machine_writes_no_hub_service() {
    let fx = Fixture::new("no-hub");
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let done = installed(install(&fx.deps(&FakeHost, &running), &config(), &head).unwrap());
    assert_eq!(done.hub, None);
    assert!(fx.service.hubs.borrow().is_empty());
    assert!(
        !installed_text(&done, Path::new("/repo"), &PathLink::Present(running)).contains("\nhub ")
    );
}

#[test]
fn on_the_hub_machine_install_writes_the_hub_service_after_the_daemons() {
    let fx = Fixture::new("hub-service").with_environment(&[
        ("IGNITE_HUB_LISTEN", "127.0.0.1:7345"),
        ("IGNITE_HUB_PUBLIC_LISTEN", "127.0.0.1:7346"),
        ("PATH", "/usr/bin"),
    ]);
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let done = installed(install(&fx.deps(&FakeHost, &running), &config(), &head).unwrap());

    assert_eq!(done.hub.as_deref(), Some("127.0.0.1:7345"));
    // The daemon's service was replaced first, freeing the address an older daemon served
    // the hub at.
    assert_eq!(
        *fx.service.hubs.borrow(),
        [HubInstall {
            executable: fx.store.current_link(),
            args: vec!["hub".to_owned()],
            environment: pairs(&[
                ("IGNITE_HUB_LISTEN", "127.0.0.1:7345"),
                ("IGNITE_HUB_PUBLIC_LISTEN", "127.0.0.1:7346"),
            ]),
            daemons_before: 1,
        }]
    );
    // The daemon's service is written the same way as on any machine.
    assert_eq!(
        *fx.service.installed.borrow(),
        [(fx.store.current_link(), vec!["daemon".to_owned()])]
    );
}

#[test]
fn install_moves_the_hub_addresses_from_the_daemons_service_to_the_hubs() {
    // Installed before the hub had a service of its own: the daemon's service carries the
    // addresses, and the shell that runs the install sets neither.
    let mut fx = Fixture::new("hub-move");
    fx.service.recorded = pairs(&[
        ("IGNITE_HUB_LISTEN", "127.0.0.1:7345"),
        ("IGNITE_HUB_PUBLIC_LISTEN", "127.0.0.1:7346"),
    ]);
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let done = installed(install(&fx.deps(&FakeHost, &running), &config(), &head).unwrap());
    assert_eq!(done.hub.as_deref(), Some("127.0.0.1:7345"));
    assert_eq!(fx.service.hubs.borrow()[0].environment, fx.service.recorded);
}

#[test]
fn the_install_environment_wins_over_the_recorded_hub_addresses() {
    let mut fx = Fixture::new("hub-env").with_environment(&[("IGNITE_HUB_LISTEN", "0.0.0.0:9")]);
    fx.service.recorded = pairs(&[
        ("IGNITE_HUB_LISTEN", "127.0.0.1:7345"),
        ("IGNITE_HUB_PUBLIC_LISTEN", "127.0.0.1:7346"),
    ]);
    assert_eq!(
        hub_settings(fx.environment.as_ref(), &fx.service),
        pairs(&[
            ("IGNITE_HUB_LISTEN", "0.0.0.0:9"),
            ("IGNITE_HUB_PUBLIC_LISTEN", "127.0.0.1:7346"),
        ])
    );
    // A public address alone is no hub.
    fx.service.recorded = pairs(&[("IGNITE_HUB_PUBLIC_LISTEN", "127.0.0.1:7346")]);
    assert!(hub_settings(&|_| None, &fx.service).is_empty());
}

#[test]
fn an_older_executable_hands_the_steps_after_the_build_to_the_new_one() {
    let fx = Fixture::new("handoff");
    let older = fx.home.join("older").join("ignite");
    script(&older, "exit 0");
    let head = commit("1234abc");
    let deps = fx.deps(&FakeHost, &older);
    let outcome = install(&deps, &config(), &head).unwrap();

    let Outcome::HandedOff(reply) = outcome else {
        panic!("the older executable finished the install itself");
    };
    assert_eq!(reply.text, "finished by the new build");
    assert_eq!(
        *fx.handoff.calls.borrow(),
        [(fx.store.path(&head), head.clone())]
    );
    assert_eq!(
        ExecutableStore::version_of(&fx.store.current_link()),
        Some(head)
    );
    // The older executable neither registered nor wrote the login services.
    assert!(fx.registrar.tokens.borrow().is_empty());
    assert!(fx.service.installed.borrow().is_empty());
    assert!(fx.service.monitors.borrow().is_empty());
}

#[test]
fn finishing_an_install_needs_the_stored_build() {
    let fx = Fixture::new("unstored");
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let error = finish_install(&fx.deps(&FakeHost, &running), &config(), &head).unwrap_err();
    assert!(error.to_string().contains("not stored"), "{error}");
    assert!(fx.service.installed.borrow().is_empty());
}

#[cfg(unix)]
#[test]
fn the_process_handoff_runs_install_in_the_stored_build() {
    let dir = tempdir("process-handoff");
    let executable = dir.join("bin").join("ignite");
    script(
        &executable,
        "echo \"$1 $IGNITE_INSTALL_BUILT $(basename \"$PWD\")\"\necho warned >&2\nexit 4",
    );
    let repo = dir.join("checkout");
    std::fs::create_dir_all(&repo).unwrap();
    let reply = ProcessHandoff { repo }.finish(&executable, &commit("1234abc"));
    assert_eq!(reply.exit, Exit::Unreachable);
    assert_eq!(reply.text, "install 1234abc checkout\nwarned");
}

#[test]
fn install_stops_before_the_service_when_the_hub_is_down() {
    let mut fx = Fixture::new("hubdown");
    fx.registrar.down = true;
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let error = install(&fx.deps(&FakeHost, &running), &config(), &head).unwrap_err();
    assert!(matches!(error, Error::Unreachable(_)));
    assert_eq!(failure(&error).exit, Exit::Unreachable);
    assert!(fx.service.installed.borrow().is_empty());
    assert!(fx.identity.load().unwrap().unwrap().token.is_none());
}

#[test]
fn install_fails_with_a_nonzero_exit_when_the_service_does_not_load() {
    let mut fx = Fixture::new("refused");
    fx.service.refuse = true;
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let error = install(&fx.deps(&FakeHost, &running), &config(), &head).unwrap_err();
    assert!(error.to_string().contains("Bootstrap failed"), "{error}");
    assert_ne!(failure(&error).exit, Exit::Success);
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
    let fx = Fixture::new("stranger");
    let head = commit("1234abc");
    let running = fx.store.path(&head);
    let error = install(&fx.deps(&Other, &running), &config(), &head).unwrap_err();
    assert!(error.to_string().contains("[machine.stranger]"), "{error}");
    assert!(fx.builder.built.borrow().is_empty());
}

fn register_request() -> MachineRegister {
    MachineRegister {
        machine: MachineId::new("m-0000abcd").unwrap(),
        name: "testbox".to_owned(),
        os: Os::Macos,
        cores: 8,
        memory_mb: 16384,
        families: vec!["rust".to_owned()],
        version: "1234abc".to_owned(),
    }
}

fn down() -> FakeRegistrar {
    FakeRegistrar {
        requests: RefCell::default(),
        tokens: RefCell::default(),
        down: true,
    }
}

#[test]
fn on_the_hub_machine_an_unreachable_hub_is_served_for_the_registration() {
    let dir = tempdir("served-hub");
    let remote = down();
    let served = ServedHub {
        config: config(),
        listen: "127.0.0.1:0".to_owned(),
        dir: dir.join("hub"),
    };
    let registrar = HubFallback {
        remote: &remote,
        local: Some(&served),
    };
    let request = register_request();
    let registered = registrar.register(&request, None).unwrap();
    assert_eq!(registered.machine, request.machine);
    assert!(!registered.token.is_empty());
    // The machine is in the database the daemon serves: registering again without its token
    // is refused.
    let again = registrar.register(&request, None).unwrap_err();
    assert!(
        again.to_string().contains("refused this machine's token"),
        "{again}"
    );
    assert!(dir.join("hub").join("hub.db").is_file());
}

#[test]
fn a_hub_that_cannot_be_served_here_reports_both_failures() {
    let dir = tempdir("busy-hub");
    // Another process, as a running daemon would, holds the hub's address.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let remote = down();
    let served = ServedHub {
        config: config(),
        listen: taken.local_addr().unwrap().to_string(),
        dir: dir.join("hub"),
    };
    let fallback = HubFallback {
        remote: &remote,
        local: Some(&served),
    };
    let error = fallback.register(&register_request(), None).unwrap_err();
    let text = error.to_string();
    assert!(matches!(error, Error::Unreachable(_)), "{text}");
    assert!(text.contains("hub is down"), "{text}");
    assert!(text.contains("cannot serve the hub"), "{text}");
    // Another machine has no hub to serve, and reports only the first failure.
    let elsewhere = HubFallback {
        remote: &remote,
        local: None,
    };
    let error = elsewhere.register(&register_request(), None).unwrap_err();
    assert_eq!(error.to_string(), "unreachable: hub is down");
}

#[test]
fn a_hub_that_answers_is_not_served_here() {
    struct Refused;
    impl Registrar for Refused {
        fn register(&self, _: &MachineRegister, _: Option<&str>) -> Result<MachineRegistered> {
            Err(Error::Invalid("not a registration".to_owned()))
        }
    }
    struct Never;
    impl LocalHub for Never {
        fn register_here(&self, _: &MachineRegister, _: Option<&str>) -> Result<MachineRegistered> {
            panic!("the hub answered, so it must not be served here");
        }
    }
    let fallback = HubFallback {
        remote: &Refused,
        local: Some(&Never),
    };
    let error = fallback.register(&register_request(), None).unwrap_err();
    assert!(matches!(error, Error::Invalid(_)));
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn the_main_checkout_of_a_linked_worktree_is_the_repository() {
    // Not canonicalized: on Windows that makes a verbatim path (`\\?\C:\...`), which git
    // cannot create a worktree under. Paths are compared canonicalized instead.
    let dir = tempdir("main-checkout");
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "a"]);
    let linked = dir.join("linked");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "fix",
            linked.to_str().unwrap(),
        ],
    );
    let repo = ignite_os::paths::canonicalize(repo).unwrap();
    assert_eq!(
        ignite_os::paths::canonicalize(main_checkout(&linked)).unwrap(),
        repo
    );
    assert_eq!(
        ignite_os::paths::canonicalize(main_checkout(&repo)).unwrap(),
        repo
    );
    // Outside any repository, the directory itself.
    let plain = dir.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(main_checkout(&plain), plain);
}

/// A home folder and the store's `current` link to an executable in it.
#[cfg(unix)]
fn linking_home(name: &str) -> (PathBuf, PathBuf) {
    let home = tempdir(name);
    let bin = home.join(".config").join("ignite").join("bin");
    script(&bin.join("1234abc").join("ignite"), "exit 0");
    let current = bin.join("current");
    std::os::unix::fs::symlink(bin.join("1234abc").join("ignite"), &current).unwrap();
    (home, current)
}

#[cfg(unix)]
#[test]
fn install_links_ignite_onto_the_path_when_local_bin_is_on_it() {
    let (home, current) = linking_home("link-path");
    let local = home.join(".local").join("bin");
    let path = std::env::join_paths(["/usr/bin".into(), local.clone()]).unwrap();
    let link = local.join("ignite");

    assert_eq!(
        link_onto_path(&current, Some(&home), Some(&path)),
        PathLink::Linked(link.clone())
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), current);
    assert_eq!(
        link_onto_path(&current, Some(&home), Some(&path)),
        PathLink::Present(link.clone())
    );
    // A link to a stored build, as made by hand, is Ignite's and is repointed.
    std::fs::remove_file(&link).unwrap();
    let stored = current.parent().unwrap().join("1234abc").join("ignite");
    std::os::unix::fs::symlink(&stored, &link).unwrap();
    assert_eq!(
        link_onto_path(&current, Some(&home), Some(&path)),
        PathLink::Linked(link.clone())
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), current);
}

#[cfg(unix)]
#[test]
fn install_says_where_to_link_ignite_when_it_cannot() {
    let (home, current) = linking_home("link-advice");
    let local = home.join(".local").join("bin");
    let link = local.join("ignite");
    // `~/.local/bin` is not on the search path: nothing is created.
    let advice = link_onto_path(&current, Some(&home), Some(OsStr::new("/usr/bin")));
    let PathLink::Advice(text) = advice else {
        panic!("linked although ~/.local/bin is not on PATH: {advice:?}");
    };
    assert!(text.contains("not on PATH"), "{text}");
    assert!(text.contains(&current.display().to_string()), "{text}");
    assert!(!link.exists());
    // A file that is not Ignite's link is left alone.
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(&link, "someone else's").unwrap();
    let path = std::env::join_paths([local.clone()]).unwrap();
    let advice = link_onto_path(&current, Some(&home), Some(&path));
    assert!(
        matches!(&advice, PathLink::Advice(text) if text.contains("not Ignite's link")),
        "{advice:?}"
    );
    assert_eq!(std::fs::read_to_string(&link).unwrap(), "someone else's");
}

#[test]
fn the_install_report_names_the_checkout_and_the_command_link() {
    let done = Installed {
        machine: MachineId::new("m-0000abcd").unwrap(),
        version: commit("1234abc"),
        executable: PathBuf::from("/home/bin/1234abc/ignite"),
        monitor: true,
        hub: Some("127.0.0.1:7345".to_owned()),
    };
    let text = installed_text(
        &done,
        Path::new("/src/repo"),
        &PathLink::Linked(PathBuf::from("/home/.local/bin/ignite")),
    );
    assert!(text.contains("checkout   /src/repo"), "{text}");
    assert!(
        text.contains("command    linked /home/.local/bin/ignite"),
        "{text}"
    );
    assert!(text.contains("hub        serves 127.0.0.1:7345"), "{text}");
}
