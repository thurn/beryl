//! macOS volume discovery and the commands used to provision an APFS volume.

use std::ffi::{CStr, OsStr, OsString};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

use ignite_core::capacity::BYTES_PER_GB;

/// Mount points from the kernel, including mounts outside /Volumes.
pub fn mount_points() -> Vec<PathBuf> {
    // SAFETY: A null buffer asks only for a count.
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    let Ok(count) = usize::try_from(count) else {
        return Vec::new();
    };
    // SAFETY: An all-zero statfs is valid storage for the kernel to fill.
    let mut mounts = vec![unsafe { std::mem::zeroed::<libc::statfs>() }; count];
    let Ok(bytes) = i32::try_from(count * size_of::<libc::statfs>()) else {
        return Vec::new();
    };
    // SAFETY: mounts is writable for bytes bytes and stays allocated through the call.
    let written = unsafe { libc::getfsstat(mounts.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    mounts.truncate(usize::try_from(written).unwrap_or(0));
    mounts
        .iter()
        .filter(|mount| {
            let flags = mount.f_flags;
            flags & libc::MNT_LOCAL as u32 != 0 && flags & libc::MNT_RDONLY as u32 == 0
        })
        .filter_map(|mount| {
            // SAFETY: c_char and u8 have the same size and alignment. The slice is bounded
            // by the mount name buffer, and the conversion below checks for NUL.
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    mount.f_mntonname.as_ptr().cast(),
                    mount.f_mntonname.len(),
                )
            };
            let name = CStr::from_bytes_until_nul(bytes).ok()?;
            Some(PathBuf::from(OsStr::from_bytes(name.to_bytes())))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct ApfsVolume {
    pub device: String,
    pub mount: PathBuf,
}

pub trait VolumeSystem {
    fn container(&self, checkout: &Path) -> io::Result<String>;
    fn matching_volume(
        &self,
        container: &str,
        name: &str,
        size_gb: u32,
    ) -> io::Result<Option<String>>;
    fn command(&self, program: &str, args: &[OsString]) -> io::Result<()>;
    fn mounted_volume(&self, device: &str) -> io::Result<ApfsVolume>;
}

pub struct NativeVolumeSystem;

impl VolumeSystem for NativeVolumeSystem {
    fn container(&self, checkout: &Path) -> io::Result<String> {
        // The Data volume may be mounted under /System/Volumes/Data even for /Users paths.
        // Resolve the actual device so those paths pick the correct container.
        let disk = Command::new("df").arg("-Pk").arg(checkout).output()?;
        let stdout = checked(disk)?;
        let text = String::from_utf8_lossy(&stdout);
        let device = text
            .lines()
            .nth(1)
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| io::Error::other("df did not report the checkout's device"))?;
        plist_string(&["info", "-plist", device], "APFSContainerReference")
    }

    fn matching_volume(
        &self,
        container: &str,
        name: &str,
        size_gb: u32,
    ) -> io::Result<Option<String>> {
        let output = plist(
            &["apfs", "list", "-plist", container],
            "Containers.0.Volumes",
            "json",
        )?;
        let volumes: Value = serde_json::from_slice(&output).map_err(io::Error::other)?;
        let volumes = volumes
            .as_array()
            .ok_or_else(|| io::Error::other("diskutil did not list APFS volumes"))?;
        let bytes = u64::from(size_gb) * BYTES_PER_GB;
        // APFS rounds the decimal size up to an allocation block.
        let matches = |value: &Value| {
            value.as_u64().is_some_and(|capacity| {
                capacity
                    .checked_sub(bytes)
                    .is_some_and(|rounding| rounding < 4096)
            })
        };
        let mut devices = volumes
            .iter()
            .filter(|volume| {
                volume["Name"].as_str() == Some(name)
                    && matches(&volume["CapacityQuota"])
                    && matches(&volume["CapacityReserve"])
            })
            .filter_map(|volume| volume["DeviceIdentifier"].as_str());
        let device = devices.next();
        if devices.next().is_some() {
            return Err(io::Error::other(format!(
                "several volumes named {name} have that size"
            )));
        }
        Ok(device.map(str::to_owned))
    }

    fn mounted_volume(&self, device: &str) -> io::Result<ApfsVolume> {
        let mount = plist_string(&["info", "-plist", device], "MountPoint")
            .ok()
            .filter(|point| point.starts_with('/'))
            .ok_or_else(|| {
                io::Error::other(format!(
                    "volume {device} is not mounted; run diskutil mount {device}"
                ))
            })?;
        Ok(ApfsVolume {
            device: device.to_owned(),
            mount: mount.into(),
        })
    }

    fn command(&self, program: &str, args: &[OsString]) -> io::Result<()> {
        checked(Command::new(program).args(args).output()?).map(|_| ())
    }
}

fn plist_string(args: &[&str], field: &str) -> io::Result<String> {
    let output = plist(args, field, "raw")?;
    Ok(String::from_utf8(output)
        .map_err(io::Error::other)?
        .trim_end_matches('\n')
        .to_owned())
}

fn plist(args: &[&str], field: &str, format: &str) -> io::Result<Vec<u8>> {
    let diskutil = checked(Command::new("/usr/sbin/diskutil").args(args).output()?)?;
    let mut plutil = Command::new("/usr/bin/plutil")
        .args(["-extract", field, format, "-o", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    plutil
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(&diskutil)?;
    checked(plutil.wait_with_output()?)
}

fn checked(output: std::process::Output) -> io::Result<Vec<u8>> {
    if output.status.success() {
        return Ok(output.stdout);
    }
    let diagnostics = [&output.stderr, &output.stdout]
        .into_iter()
        .map(|bytes| String::from_utf8_lossy(bytes).trim().to_owned())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Err(io::Error::other(if diagnostics.is_empty() {
        output.status.to_string()
    } else {
        diagnostics
    }))
}
