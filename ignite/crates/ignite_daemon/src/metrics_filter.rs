//! Which rows of an efficiency export the daemon posts to the hub (see [`hub_keeps`]).

/// Whether the hub needs an export row of the table `kind`. On the hub's own machine the
/// efficiency dashboard reads this machine's transcripts itself and skips its rows in the hub,
/// so only the rows the hub reads itself are posted: the model windows' dollars, which the
/// backlog ratios sum. Elsewhere every row is posted but those of the tables the dashboard
/// rebuilds from the hub's own records (`ignite_store::hub::metrics::REBUILT_KINDS`). Before,
/// every row was posted, and the hub's `metrics` table grew by about 1.5 GB a day, most of it
/// rows nothing read.
pub(super) fn hub_keeps(kind: Option<&str>, on_hub_machine: bool) -> bool {
    match kind {
        None => true,
        Some(kind) if on_hub_machine => kind == "window_dollars",
        Some(kind) => !ignite_store::hub::metrics::REBUILT_KINDS.contains(&kind),
    }
}

/// The host name of the hub's machine as an export names hosts: the first label of the host in
/// the hub's address, in lower case (`https://alpha.example.ts.net:8443` gives `alpha`).
pub(super) fn hub_host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', ':']).next()?;
    let label = host.split('.').next()?;
    (!label.is_empty() && !label.chars().all(|c| c.is_ascii_digit()))
        .then(|| label.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hub_host_is_the_first_label_of_its_address() {
        assert_eq!(
            hub_host("https://Alpha.example.ts.net:8443").as_deref(),
            Some("alpha")
        );
        assert_eq!(hub_host("http://beta/x").as_deref(), Some("beta"));
        assert_eq!(hub_host("http://127.0.0.1:7345"), None);
    }

    #[test]
    fn the_hub_machine_posts_only_window_dollars() {
        assert!(hub_keeps(None, true));
        assert!(hub_keeps(Some("window_dollars"), true));
        assert!(!hub_keeps(Some("tool_calls"), true));
        assert!(!hub_keeps(Some("compile_timings"), true));
    }

    #[test]
    fn other_machines_leave_out_the_tables_rebuilt_from_records() {
        assert!(hub_keeps(Some("tool_calls"), false));
        assert!(hub_keeps(Some("window_dollars"), false));
        for kind in ignite_store::hub::metrics::REBUILT_KINDS {
            assert!(!hub_keeps(Some(kind), false), "{kind}");
        }
    }
}
