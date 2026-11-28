//! Provision the volume and report steps that need the operator's administrator privileges.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};

use ignite_core::protocol::client::{Exit, Reply};
use ignite_daemon::doctor::{Host, NativeHost};
use ignite_os::system::{NativeVolumeSystem, VolumeSystem};

use crate::fleet::FleetCommand;
use crate::ticket::http::usage;

struct Provisioned {
    folder: PathBuf,
    notes: Vec<String>,
    folder_ready: bool,
}

pub(super) fn run(cwd: &Path, state: bool, size_gb: u32) -> Reply {
    let (checkout, config) = match crate::config_above(cwd) {
        Ok(pair) => pair,
        Err(failure) => return failure.into_reply(),
    };
    let repo = ignite_daemon::update::main_checkout(&checkout);
    let project = repo
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&config.hub.project);
    let mut name = project.to_owned();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    name.push_str(if state { "State" } else { "Trees" });

    let home = ignite_daemon::update::Home::from_env();
    let prepared = (|| {
        home.create()?;
        // The check for an existing quota and diskutil's creation must run together.
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(home.root.join("volume.lock"))?;
        lock.lock()?;
        provision(&NativeVolumeSystem, &repo, &name, state, size_gb)
    })();
    let volume = match prepared {
        Ok(volume) => volume,
        Err(error) => return usage(error).into_reply(),
    };
    let mut text = format!(
        "Prepared {name} ({size_gb} GB) at {}",
        volume.folder.display()
    );
    for note in &volume.notes {
        let _ = write!(text, "\n{note}");
    }
    if !volume.folder_ready {
        let _ = write!(
            text,
            "\nRun the directory setup command shown above, then run this command again."
        );
        return Reply::text(Exit::Usage, text);
    }
    if state {
        let _ = write!(text, "\n{}", state_instructions(&volume.folder, &home.root));
        return Reply::text(Exit::Success, text);
    }

    let root = volume.folder.display().to_string();
    let host = NativeHost.hostname();
    let command = FleetCommand::Machine {
        name: host.clone(),
        state: None,
        for_hours: None,
        indefinitely: false,
        trees_root: Some(root.clone()),
        tree_space_gb: None,
        speed: None,
        tree_ceiling_gb: None,
    };
    let reply = crate::fleet::run(Some(&command), cwd);
    let _ = write!(text, "\n{}", reply.text);
    if reply.exit != Exit::Success {
        let _ = write!(
            text,
            "\nTo save the root, run: ignite fleet machine {} --trees-root {}",
            shell_word(&host),
            shell_word(&root)
        );
    }
    Reply::text(reply.exit, text)
}

fn provision(
    system: &dyn VolumeSystem,
    repo: &Path,
    name: &str,
    state: bool,
    size_gb: u32,
) -> io::Result<Provisioned> {
    let container = system.container(repo)?;
    let device = if let Some(device) = system.matching_volume(&container, name, size_gb)? {
        device
    } else {
        let size = format!("{size_gb}g");
        let args = [
            "apfs",
            "addVolume",
            &container,
            "APFS",
            name,
            "-reserve",
            &size,
            "-quota",
            &size,
        ]
        .map(OsString::from);
        system.command("diskutil", &args).map_err(|error| {
                io::Error::other(format!(
                    "Cannot create the volume: {error}\nIf macOS requires administrator privileges, run: {}\nThen run this command again.",
                    sudo_command("diskutil", &args)
                ))
            })?;
        system
            .matching_volume(&container, name, size_gb)?
            .ok_or_else(|| io::Error::other("diskutil did not report the created volume"))?
    };
    let volume = system.mounted_volume(&device)?;
    let folder = volume.mount.join(if state { "state" } else { "trees" });
    let markers = volume.mount.join(".fseventsd");
    let mut notes = Vec::new();
    attempt(
        system,
        &mut notes,
        "diskutil",
        &["enableOwnership".into(), volume.device.into()],
    );
    attempt(
        system,
        &mut notes,
        "mkdir",
        &["-p".into(), markers.clone().into_os_string()],
    );
    attempt(
        system,
        &mut notes,
        "touch",
        &[
            markers.join("no_log").into_os_string(),
            volume.mount.join(".metadata_never_index").into_os_string(),
        ],
    );
    attempt(
        system,
        &mut notes,
        "tmutil",
        &[
            "addexclusion".into(),
            "-v".into(),
            volume.mount.into_os_string(),
        ],
    );
    let folder_path = folder.clone().into_os_string();
    let folder_ready = match system
        .command("mkdir", &["-p".into(), folder_path.clone()])
        .and_then(|()| system.command("test", &["-w".into(), folder_path]))
    {
        Ok(()) => true,
        Err(error) => {
            notes.push(format!(
                "Cannot write to {}: {error}\nRun: sudo install -d -o \"$(id -u)\" -g \"$(id -g)\" {}",
                folder.display(), shell_word(&folder.display().to_string())
            ));
            false
        }
    };
    Ok(Provisioned {
        folder,
        notes,
        folder_ready,
    })
}

fn attempt(system: &dyn VolumeSystem, notes: &mut Vec<String>, program: &str, args: &[OsString]) {
    if let Err(error) = system.command(program, args) {
        notes.push(format!("{error}\nRun: {}", sudo_command(program, args)));
    }
}

fn sudo_command(program: &str, args: &[OsString]) -> String {
    let args = args
        .iter()
        .map(|arg| shell_word(&arg.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    format!("sudo {program} {args}")
}

fn shell_word(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

fn state_instructions(folder: &Path, home: &Path) -> String {
    let destination = shell_word(&folder.join("ignite").display().to_string());
    let home = shell_word(&home.display().to_string());
    format!(
        "Run these commands from a shell to stop the services and move the state:\n\
         for service in ignite.monitor ignite.daemon ignite.hub; do launchctl bootout \"gui/$(id -u)/$service\"; done; for service in ignite.monitor ignite.daemon ignite.hub; do if launchctl print \"gui/$(id -u)/$service\" >/dev/null 2>&1; then echo \"Still running: $service\"; fi; done\n\
         After all services stop, run:\n\
         test -d {home} && test ! -L {home} && test ! -e {destination} && test ! -L {destination} && mv -- {home} {destination} && ln -s -- {destination} {home}\n\
         Run ignite install from the main checkout to start the services again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_os::system::ApfsVolume;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeSystem {
        commands: RefCell<Vec<(String, Vec<OsString>)>>,
    }

    impl VolumeSystem for FakeSystem {
        fn container(&self, _: &Path) -> io::Result<String> {
            Ok("disk7".to_owned())
        }

        fn matching_volume(&self, _: &str, _: &str, _: u32) -> io::Result<Option<String>> {
            Ok((!self.commands.borrow().is_empty()).then(|| "disk7s2".to_owned()))
        }

        fn mounted_volume(&self, device: &str) -> io::Result<ApfsVolume> {
            Ok(ApfsVolume {
                device: device.to_owned(),
                mount: "/Volumes/Test Trees".into(),
            })
        }

        fn command(&self, program: &str, args: &[OsString]) -> io::Result<()> {
            self.commands
                .borrow_mut()
                .push((program.to_owned(), args.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn volume_sets_reserve_quota_markers_and_backup_exclusion() {
        let fake = FakeSystem::default();
        let volume = provision(&fake, Path::new("/checkout"), "TestTrees", false, 60).unwrap();
        assert!(volume.folder_ready && volume.notes.is_empty());
        assert_eq!(volume.folder, Path::new("/Volumes/Test Trees/trees"));
        let commands = fake.commands.borrow();
        assert_eq!(
            commands[0],
            (
                "diskutil".into(),
                [
                    "apfs",
                    "addVolume",
                    "disk7",
                    "APFS",
                    "TestTrees",
                    "-reserve",
                    "60g",
                    "-quota",
                    "60g"
                ]
                .map(OsString::from)
                .to_vec()
            )
        );
        assert_eq!(
            commands[3],
            (
                "touch".into(),
                [
                    "/Volumes/Test Trees/.fseventsd/no_log",
                    "/Volumes/Test Trees/.metadata_never_index"
                ]
                .map(OsString::from)
                .to_vec()
            )
        );
        assert_eq!(
            commands[4],
            (
                "tmutil".into(),
                ["addexclusion", "-v", "/Volumes/Test Trees"]
                    .map(OsString::from)
                    .to_vec()
            )
        );
    }
}
