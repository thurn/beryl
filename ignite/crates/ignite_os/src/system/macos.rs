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
