//! Report when the macOS file event daemon's resident memory grows into the gigabytes.

use super::Finding;

/// A healthy daemon uses tens of MiB. Warn when more than 1 GiB is resident.
const WARNING_MIB: u64 = 1024;

pub(super) fn finding(rss_mib: u64) -> Finding {
    let reading = format!("fseventsd has {rss_mib} MiB resident");
    if rss_mib > WARNING_MIB {
        Finding::warning(
            "fseventsd",
            format!("{reading}, above the {WARNING_MIB} MiB threshold."),
            "Run `sudo killall fseventsd` on this Mac to reclaim the memory. macOS restarts it.",
        )
    } else {
        Finding::ok("fseventsd", format!("{reading}."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_only_above_one_gib_with_the_restart_command() {
        for rss_mib in [64, 1024] {
            let result = finding(rss_mib);
            assert!(result.ok);
            assert!(!result.warning);
            assert!(result.line().contains(&format!("{rss_mib} MiB resident")));
        }

        let result = finding(1025);
        assert!(result.ok);
        assert!(result.warning);
        let line = result.line();
        assert!(line.contains("1025 MiB resident, above the 1024 MiB threshold"));
        assert!(line.contains("sudo killall fseventsd"));
    }
}
