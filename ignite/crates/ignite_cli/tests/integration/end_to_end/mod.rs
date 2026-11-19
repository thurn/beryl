// The daemon and its socket are Unix-only until `ignite_daemon` builds on Windows, and the
// checks' adapter is a shell script.
#![cfg(unix)]

//! One ticket through the whole system: real `git`, a real hub on a random port and daemons
//! assembled in the test's process, on a temporary repository (see `world`).

mod world;

use std::process::Command;
use std::sync::Arc;

use ignite_core::protocol::tickets::{Status, TicketQuery};
use ignite_daemon::tip::TipOutcome;

use world::{World, ask, commit, detail, start, ticket};

/// `ignite start`, an edit, `ignite test`, a commit and `ignite land`, sent through the real
/// `ignite` executable and the daemon's socket. The commit reaches `staging`, the ticket is
/// closed with its note, and `master` moves once the tip is verified.
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
            .env("IGNITE_SESSION", "local_e2e1")
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

/// A change that breaks a check the gate leaves out (its score is below the threshold) lands,
/// and tip verification files a regression ticket that names the check.
#[test]
fn a_change_that_breaks_a_check_below_the_threshold_files_a_regression() {
    let world = World::new("regress");
    let daemon = world.daemon("alpha");
    let ticket = ticket(&daemon, "Beta breaks");
    let tree = start(&daemon, "local_e2e4", &ticket);
    commit(
        &tree,
        "crates/beta/src/lib.rs",
        "\n// BROKEN\n",
        "fix: beta, quietly broken",
    );
    let reply = ask(
        &daemon,
        "local_e2e4",
        &tree,
        "land",
        &["-m", "Breaks beta."],
    );
    assert_eq!(reply.exit.code(), 0, "{}", reply.text);

    let master = world.remote_tip("master");
    let outcome = daemon.verify_tip().unwrap();
    let failed = match outcome {
        TipOutcome::Failed { checks } => checks,
        other => panic!("the tip should fail, not {other:?}"),
    };
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].as_str(), "extra:beta_adds");
    assert_eq!(world.remote_tip("master"), master, "master holds");

    let all = TicketQuery {
        all: true,
        ..TicketQuery::default()
    };
    let tickets = daemon.ctx.hub.tickets(&all).unwrap();
    let regression = tickets
        .iter()
        .find(|t| t.title.contains("extra:beta_adds"))
        .unwrap_or_else(|| panic!("no regression ticket among {tickets:?}"));
    assert_eq!(
        regression.status,
        Status::Open,
        "a regression holds master back, so it is filed released"
    );
}
