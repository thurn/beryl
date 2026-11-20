//! `ignite pool watch <ticket | session>`: prints the digest of a CLI worker's transcript and
//! follows it until interrupted. The daemon answers `pool watch --after <n>` with the digest
//! lines after the first `n` and the number of lines there are now, so each poll prints only
//! what is new. `--no-follow` prints what exists and exits.
//!
//! A daemon that restarts while the digest is followed is waited for, since the follower has
//! already shown that the session exists.

use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use ignite_core::protocol::client::{Exit, Reply};

use crate::client;

/// Seconds between two polls of a followed digest.
const POLL_S: u64 = 1;

pub fn run(target: &str, no_follow: bool, cwd: &Path) -> Reply {
    let session = client::session_from_env(|name| std::env::var(name).ok());
    let mut send = |after: usize| {
        let mut request = client::request("pool", &[], cwd.to_path_buf(), session.clone());
        request.args = vec![
            "watch".to_owned(),
            target.to_owned(),
            "--after".to_owned(),
            after.to_string(),
        ];
        client::send(&request)
    };
    let mut print = |text: &str| {
        let mut out = std::io::stdout().lock();
        // A reader that has gone (a closed pipe) ends the watch quietly.
        if writeln!(out, "{text}").and_then(|()| out.flush()).is_err() {
            std::process::exit(0);
        }
    };
    let mut pause = || std::thread::sleep(Duration::from_secs(POLL_S));
    follow(&mut send, no_follow, &mut print, &mut pause)
}

/// Polls `send` (which asks for the lines after the given count) and prints each reply's text.
/// Returns the first reply that failed, when nothing has been printed yet, or an empty success
/// when `no_follow` is set. Never returns otherwise.
fn follow(
    send: &mut dyn FnMut(usize) -> Reply,
    no_follow: bool,
    print: &mut dyn FnMut(&str),
    pause: &mut dyn FnMut(),
) -> Reply {
    let mut after = 0;
    let mut shown = false;
    loop {
        let reply = send(after);
        if reply.exit == Exit::Success {
            if !reply.text.is_empty() {
                print(reply.text.trim_end());
            }
            if let Some(next) = reply
                .json
                .as_ref()
                .and_then(|json| json.get("next"))
                .and_then(serde_json::Value::as_u64)
            {
                after = usize::try_from(next).unwrap_or(after);
            }
            shown = true;
        } else if !shown || no_follow {
            return reply;
        }
        if no_follow {
            return Reply::text(Exit::Success, "");
        }
        pause();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(next: u64, text: &str) -> Reply {
        Reply {
            exit: Exit::Success,
            text: text.to_owned(),
            json: Some(serde_json::json!({"next": next})),
            handle: None,
        }
    }

    #[test]
    fn no_follow_prints_what_exists_and_exits() {
        let mut printed = Vec::new();
        let reply = follow(
            &mut |after| {
                assert_eq!(after, 0);
                lines(2, "say      hello")
            },
            true,
            &mut |text| printed.push(text.to_owned()),
            &mut || panic!("no pause without following"),
        );
        assert_eq!(reply.exit, Exit::Success);
        assert_eq!(printed, ["say      hello"]);
    }

    #[test]
    fn following_asks_after_what_it_has_shown_and_survives_a_failed_poll() {
        let mut asked = Vec::new();
        let mut printed = Vec::new();
        let mut pauses = 0;
        let mut script = vec![
            lines(2, "turn     1\nsay      a"),
            Reply::text(Exit::Usage, "the daemon did not answer"),
            lines(3, "tool     Bash: ls"),
        ]
        .into_iter();
        // The script ends by panicking out of the loop, which `catch_unwind` turns back into a
        // value, so the test needs no clock and no way to interrupt the follower.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            follow(
                &mut |after| {
                    asked.push(after);
                    script.next().expect("the script ended")
                },
                false,
                &mut |text| printed.push(text.to_owned()),
                &mut || pauses += 1,
            )
        }));
        assert!(outcome.is_err());
        assert_eq!(asked, [0, 2, 2, 3]);
        assert_eq!(printed, ["turn     1\nsay      a", "tool     Bash: ls"]);
        assert_eq!(pauses, 3);
    }

    #[test]
    fn a_session_that_does_not_exist_is_the_error() {
        let reply = follow(
            &mut |_| Reply::text(Exit::Usage, "no CLI session works or worked T.9"),
            false,
            &mut |_| panic!("nothing to print"),
            &mut || panic!("no pause"),
        );
        assert_eq!(reply.exit, Exit::Usage);
    }
}
