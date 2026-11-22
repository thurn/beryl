//! Identifies mounted volumes that contain only build trees and normal filesystem metadata.

use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

use ignite_core::fleet::{TreeVolume, TreeVolumes};

use super::ticket_tree_number;

pub(crate) fn report(default_root: &Path) -> TreeVolumes {
    let dedicated = ignite_os::system::mount_points()
        .into_iter()
        .filter_map(|mount| inspect(&mount))
        .collect();
    TreeVolumes {
        default_root: default_root.display().to_string(),
        dedicated,
    }
}

fn inspect(mount: &Path) -> Option<TreeVolume> {
    let contents = children(mount, volume_metadata)?;
    let root = match contents.as_slice() {
        [] => None,
        trees if trees.iter().all(is_tree) => Some(mount.display().to_string()),
        [folder]
            if folder.file_type().ok()?.is_dir()
                && children(&folder.path(), staging_metadata)?
                    .iter()
                    .all(is_tree) =>
        {
            Some(folder.path().display().to_string())
        }
        _ => return None,
    };
    Some(TreeVolume {
        mount: mount.display().to_string(),
        root,
    })
}

fn children(path: &Path, ignore: fn(&str) -> bool) -> Option<Vec<DirEntry>> {
    let entries = fs::read_dir(path)
        .ok()?
        .collect::<std::io::Result<Vec<_>>>()
        .ok()?;
    Some(
        entries
            .into_iter()
            .filter(|entry| !entry.file_name().to_str().is_some_and(ignore))
            .collect(),
    )
}

fn volume_metadata(name: &str) -> bool {
    matches!(
        name,
        ".fseventsd"
            | ".Spotlight-V100"
            | ".Trashes"
            | ".TemporaryItems"
            | ".DocumentRevisions-V100"
            | ".metadata_never_index"
            | ".DS_Store"
            | "System Volume Information"
            | "$RECYCLE.BIN"
    ) || staging_metadata(name)
}

fn staging_metadata(name: &str) -> bool {
    name.strip_prefix('.').is_some_and(|rest| {
        rest.strip_suffix(".seeding")
            .is_some_and(|id| ticket_tree_number(id).is_some())
            || rest
                .strip_suffix(".commands.lock")
                .is_some_and(is_tree_name)
            || name.starts_with(".discarded.")
    })
}

fn is_tree_name(name: &str) -> bool {
    ticket_tree_number(name).is_some()
        || name.strip_prefix("landing-").is_some_and(|number| {
            number
                .parse::<u32>()
                .is_ok_and(|id| id > 0 && number == id.to_string())
        })
}

fn is_tree(entry: &DirEntry) -> bool {
    entry.file_name().to_str().is_some_and(is_tree_name)
        && entry.file_type().is_ok_and(|kind| kind.is_dir())
        && entry.path().join(".git").is_file()
}

#[cfg(target_os = "macos")]
pub(crate) fn mac_volume_is_shared(root: &Path) -> bool {
    let root = canonical_with_missing_suffix(root);
    let Some(mount) = ignite_os::system::mount_points()
        .into_iter()
        .filter(|mount| root.starts_with(mount))
        .max_by_key(|mount| mount.components().count())
    else {
        return true;
    };
    let Some(volume) = inspect(&mount) else {
        return true;
    };
    let report = TreeVolumes {
        default_root: String::new(),
        dedicated: vec![volume],
    };
    !report.allows_auto(root.to_str(), ignite_core::config::Os::Macos)
}

/// A configured root may not exist yet. Resolve its deepest existing ancestor without
/// treating a mount point reached via a symlink as part of the caller's original volume.
#[cfg(target_os = "macos")]
fn canonical_with_missing_suffix(path: &Path) -> PathBuf {
    path.ancestors()
        .find_map(|ancestor| {
            let resolved = ignite_os::paths::canonicalize(ancestor).ok()?;
            Some(resolved.join(path.strip_prefix(ancestor).ok()?))
        })
        .unwrap_or_else(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_locks_do_not_make_a_tree_volume_shared() {
        let mount = ignite_core::test_files::temp_dir()
            .join(format!("ignite-tree-volume-{}", std::process::id()));
        let root = mount.join("trees");
        let tree = root.join("tree-01");
        fs::create_dir_all(&tree).unwrap();
        fs::write(tree.join(".git"), "gitdir: fixture").unwrap();
        let command = super::super::command_lock(&tree).unwrap();
        assert_eq!(inspect(&mount).unwrap().root.as_deref(), root.to_str());
        fs::write(root.join(".other.commands.lock"), "other data").unwrap();
        assert!(inspect(&mount).is_none());
        drop(command);
        fs::remove_dir_all(mount).unwrap();
    }
}
