//! The volume size and available space, from the same `df` sample.

use std::io;
use std::path::Path;
use std::process::Command;

use super::DiskSpace;

pub fn disk_space(path: &Path) -> io::Result<DiskSpace> {
    let output = Command::new("df").arg("-Pk").arg(path).output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    parse_df(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cannot read df volume sizes"))
}

/// When the machine last started, in milliseconds since the Unix epoch, from
/// `sysctl -n kern.boottime`.
pub fn booted_at_millis() -> io::Result<i64> {
    let output = Command::new("/usr/sbin/sysctl")
        .args(["-n", "kern.boottime"])
        .output()?;
    parse_boottime(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "cannot read kern.boottime"))
}

/// `{ sec = 1764000000, usec = 250000 } Tue Nov 25 ...` in milliseconds.
fn parse_boottime(text: &str) -> Option<i64> {
    let field = |name: &str| -> Option<i64> {
        let (_, rest) = text.split_once(&format!("{name} = "))?;
        rest.split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    };
    Some(field("sec")? * 1000 + field("usec")? / 1000)
}

fn parse_df(text: &str) -> Option<DiskSpace> {
    let mut fields = text.lines().nth(1)?.split_whitespace();
    let kilobytes = |field: &str| field.parse::<u64>().ok()?.checked_mul(1024);
    let total = kilobytes(fields.nth(1)?)?;
    let available = kilobytes(fields.nth(1)?)?;
    Some(DiskSpace { total, available })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_boot_time_in_milliseconds() {
        assert_eq!(
            parse_boottime("{ sec = 1764000000, usec = 250000 } Tue Nov 25 06:40:00 2025\n"),
            Some(1_764_000_000_250)
        );
        assert_eq!(parse_boottime(""), None);
    }

    #[test]
    fn reads_total_and_available_from_the_same_volume() {
        let space = parse_df(
            "Filesystem 1024-blocks Used Available Capacity Mounted on
/dev/disk3s5 1000 800 150 85% /System/Volumes/Data",
        );
        assert_eq!(
            space,
            Some(DiskSpace {
                total: 1_024_000,
                available: 153_600
            })
        );
    }
}

#[cfg(target_os = "macos")]
mod volumes;

#[cfg(target_os = "macos")]
pub use volumes::{ApfsVolume, NativeVolumeSystem, VolumeSystem, mount_points};
