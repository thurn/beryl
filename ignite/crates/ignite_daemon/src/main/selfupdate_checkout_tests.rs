//! A restart into a release waits until the main checkout holds the release, so that the new
//! daemon reads the release's `ignite.toml` (see `selfupdate_checkout.rs`).

use ignite_core::fakes::FakeClock;
use ignite_core::time::Timestamp;

use super::*;

/// A main checkout that always holds the release, so that it never holds a restart back.
pub(super) struct UpToDate;

impl MainCheckout for UpToDate {
    fn bring_up_to(&self, _release: &Commit) -> Result<bool> {
        Ok(true)
    }
}

/// A main checkout whose `ignite.toml` is a real file. It has `OLD` checked out and the remote
/// has `NEW`, whose `ignite.toml` differs, as when the `ignite-main` loop has not fast-forwarded
/// it yet. While `edited` says it has local edits, a fast-forward moves nothing.
struct Checkout {
    toml: PathBuf,
    head: Mutex<Commit>,
    edited: Mutex<bool>,
}

const OLD_TOML: &str = "[repository]\nstaging = \"staging\"\n";
const NEW_TOML: &str = "[repository]\nstaging = \"refs/ignite/staging\"\n";

impl Checkout {
    fn behind(dir: &Path) -> Arc<Checkout> {
        let toml = dir.join("ignite.toml");
        std::fs::write(&toml, OLD_TOML).unwrap();
        Arc::new(Checkout {
            toml,
            head: Mutex::new(commit(OLD)),
            edited: Mutex::new(false),
        })
    }
}

impl MainCheckout for Checkout {
    fn bring_up_to(&self, release: &Commit) -> Result<bool> {
        let mut head = self.head.lock().unwrap();
        if !*self.edited.lock().unwrap() {
            std::fs::write(&self.toml, NEW_TOML)?;
            *head = commit(NEW);
        }
        Ok(*head == *release)
    }
}

/// Reads the checkout's `ignite.toml` at each restart, as the new daemon does when it starts.
#[derive(Clone)]
struct ReadsToml {
    toml: PathBuf,
    read: Arc<Mutex<Vec<String>>>,
}

impl Restart for ReadsToml {
    fn restart(&self, _executable: &Path, _args: &[String]) -> io::Result<()> {
        let text = std::fs::read_to_string(&self.toml)?;
        self.read.lock().unwrap().push(text);
        Ok(())
    }
}

/// A daemon on `OLD` with the release `NEW` built and ready, which changes the daemon's code
/// and `ignite.toml`, and a main checkout still at `OLD`.
fn ready_with_checkout_behind() -> (SelfUpdate, Arc<Checkout>, ReadsToml, Arc<FakeClock>) {
    let (mut update, git, _) = daemon(NEW);
    git.set_changed(
        &commit(OLD),
        &commit(NEW),
        &["ignite.toml", "ignite/crates/ignite_daemon/src/x.rs"],
    );
    let checkout = Checkout::behind(&tempdir());
    let restart = ReadsToml {
        toml: checkout.toml.clone(),
        read: Arc::default(),
    };
    let clock = Arc::new(FakeClock::new(Timestamp::from_millis(0)));
    update.checkout = checkout.clone();
    update.restart = Box::new(restart.clone());
    update.clock = clock.clone();
    update.after_master_moved().unwrap();
    settle(&update);
    (update, checkout, restart, clock)
}

#[test]
fn a_restart_into_a_release_that_changes_ignite_toml_reads_the_new_file() {
    let (update, _checkout, restart, _clock) = ready_with_checkout_behind();
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(*restart.read.lock().unwrap(), [NEW_TOML]);
}

#[test]
fn a_restart_waits_for_a_checkout_with_local_edits_until_it_is_fast_forwarded() {
    let (update, checkout, restart, clock) = ready_with_checkout_behind();
    *checkout.edited.lock().unwrap() = true;
    update.after_heartbeat(Some(OLD), IDLE);
    clock.advance(60.0);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.read.lock().unwrap().is_empty());

    *checkout.edited.lock().unwrap() = false;
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(*restart.read.lock().unwrap(), [NEW_TOML]);
}

#[test]
fn a_checkout_that_never_holds_the_release_holds_the_restart_only_up_to_the_cap() {
    let (update, checkout, restart, clock) = ready_with_checkout_behind();
    *checkout.edited.lock().unwrap() = true;
    update.after_heartbeat(Some(OLD), IDLE);
    clock.advance(WINDOW.cap_seconds - 1.0);
    update.after_heartbeat(Some(OLD), IDLE);
    assert!(restart.read.lock().unwrap().is_empty());
    clock.advance(1.0);
    update.after_heartbeat(Some(OLD), IDLE);
    assert_eq!(*restart.read.lock().unwrap(), [OLD_TOML]);
}
