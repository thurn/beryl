//! One ticket through the whole system: real `git`, a real hub on a random port and daemons
//! assembled in the test's process, on a temporary repository (see `world`).

mod world;

use std::process::Command;
use std::sync::Arc;

use ignite_core::capacity::{MachineFacts, derive};
use ignite_core::config::Os;
use ignite_core::protocol::tickets::Status;
use ignite_daemon::tip::TipOutcome;
use ignite_os::hostcpu::HostCpu;

use world::{World, ask, commit, detail, start, ticket};

#[test]
fn the_daemon_starts_without_machine_configuration() {
    let world = World::new("defaults");
    let repo = world.checkout("alpha");
    let config = repo.join("ignite.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    let (without_machines, _) = text.split_once("[machine.alpha]").unwrap();
    std::fs::write(config, without_machines).unwrap();
    let daemon = world.daemon_from("alpha", repo);
    assert!(daemon.ctx.config.machine.is_empty());
    let trees = daemon.options.repo.join(".trees");
    let expected = derive(&MachineFacts {
        os: Os::current().unwrap(),
        physical_cores: ignite_os::Native.physical_cores(),
        logical_cores: ignite_os::Native.cores(),
        memory_mb: 0,
        disk_total_gb: 0,
    })
    .landing_trees;
    for index in 1..=expected {
        assert!(trees.join(format!("landing-{index}")).is_dir());
    }
    assert!(!trees.join(format!("landing-{}", expected + 1)).exists());
}

/// A Codex thread uses the real executable to start, test and land a ticket over the daemon's
/// socket. The commit reaches `staging`, the ticket closes with its note, and `master` moves
/// once the tip is verified.
#[test]
fn a_ticket_lands_and_master_moves_once_the_tip_is_verified() {
    let world = World::new("one");
    let daemon = world.daemon("alpha");
    let ticket = ticket(&daemon, "Alpha learns to add three numbers");
    let home = daemon.options.home.root.clone();
    let ignite = |cwd: &std::path::Path, args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ignite"));
        command
            .args(args)
            .current_dir(cwd)
            .env("IGNITE_HOME", &home)
            .env("CODEX_THREAD_ID", "0196857d-2f65-709b-b98e-bf84c11aa637")
            .env_remove("IGNITE_SESSION")
            .env_remove("CLAUDE_CODE_HOST_SESSION_ID");
        let output = command.output().unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    };

    let (code, text) = ignite(&daemon.options.repo, &["--json", "start", ticket.as_str()]);
    assert_eq!(code, Some(0), "{text}");
    let started: serde_json::Value = serde_json::from_str(&text).unwrap();
    let tree = std::path::PathBuf::from(started["path"].as_str().unwrap());

    commit(
        &tree,
        "crates/alpha/src/lib.rs",
        "\npub fn add3(a: u32, b: u32, c: u32) -> u32 {\n    add(add(a, b), c)\n}\n",
        "feat: add three numbers",
    );
    let (code, text) = ignite(&tree, &["test", "core:alpha_adds"]);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("all passed"), "{text}");

    let before = world.remote_tip("master");
    let (code, text) = ignite(&tree, &["land", "-m", "Alpha adds three numbers."]);
    assert_eq!(code, Some(0), "{text}");
    assert!(text.contains("landed"), "{text}");
    assert_eq!(world.subjects("staging")[0], "feat: add three numbers");
    let detail = detail(&daemon, &ticket);
    assert_eq!(detail.ticket.status, Status::Closed);
    let landed = detail.ticket.landed.expect("the hub records the landing");
    assert_eq!(landed.note, "Alpha adds three numbers.");
    assert_eq!(
        world.remote_tip("master"),
        before,
        "master waits for the tip"
    );

    assert_eq!(daemon.verify_tip().unwrap(), TipOutcome::Verified);
    assert_eq!(world.remote_tip("master"), world.remote_tip("staging"));
}

/// A second daemon, with a machine ID of its own, takes the first one's result for the same
/// inputs from the hub's cache instead of running the check again.
#[test]
fn a_second_machine_reuses_the_first_ones_results() {
    let world = World::new("cache");
    let first = world.daemon("alpha");
    let second = world.daemon("beta");
    assert_ne!(first.ctx.machine, second.ctx.machine);

    let one = ticket(&first, "Check alpha on the first machine");
    let tree = start(&first, "local_e2e2a", &one);
    let reply = ask(&first, "local_e2e2a", &tree, "test", &["core:alpha_adds"]);
    assert_eq!(reply.exit.code(), 0, "{}", reply.text);
    assert!(reply.text.contains("all passed"), "{}", reply.text);

    let two = ticket(&second, "Check alpha on the second machine");
    let tree = start(&second, "local_e2e2b", &two);
    let why = ask(
        &second,
        "local_e2e2b",
        &tree,
        "why",
        &["core:alpha_adds", "--inputs"],
    );
    let by_first = format!("passed on {}", first.ctx.machine);
    assert!(why.text.contains(&by_first), "{}", why.text);
    let reply = ask(&second, "local_e2e2b", &tree, "test", &["core:alpha_adds"]);
    assert_eq!(reply.exit.code(), 0, "{}", reply.text);
    assert!(reply.text.contains("from cache"), "{}", reply.text);
}

/// Two tickets that land at once both reach `staging`: the second rebases onto the first.
#[test]
fn two_landings_at_once_both_land() {
    let world = World::new("two");
    let daemon = world.daemon("alpha");
    let tickets = [
        (ticket(&daemon, "Alpha doubles"), "local_e2e3a"),
        (ticket(&daemon, "Beta triples"), "local_e2e3b"),
    ];
    let edits = [
        (
            "crates/alpha/src/lib.rs",
            "\npub fn double(a: u32) -> u32 {\n    a * 2\n}\n",
        ),
        (
            "crates/beta/src/lib.rs",
            "\npub fn triple(a: u32) -> u32 {\n    a * 3\n}\n",
        ),
    ];
    let mut landings = Vec::new();
    for ((ticket, session), (file, text)) in tickets.iter().zip(edits) {
        let tree = start(&daemon, session, ticket);
        let subject = format!("feat: {}", ticket.as_str());
        commit(&tree, file, text, &subject);
        let (daemon, session) = (Arc::clone(&daemon), *session);
        landings.push(std::thread::spawn(move || {
            ask(&daemon, session, &tree, "land", &["-m", "Landed at once."])
        }));
    }
    for landing in landings {
        let reply = landing.join().unwrap();
        assert_eq!(reply.exit.code(), 0, "{}", reply.text);
    }
    let subjects = world.subjects("staging");
    for (ticket, _) in &tickets {
        let subject = format!("feat: {}", ticket.as_str());
        assert!(subjects.contains(&subject), "{subject} not in {subjects:?}");
        assert_eq!(detail(&daemon, ticket).ticket.status, Status::Closed);
    }
}
