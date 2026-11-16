//! Saved bug reports: finding one by its ID and zipping it to share. The report folder is the
//! durable record even if filing fails.

use std::path::{Path, PathBuf};

use beryl_session::{SessionError, bugs};

/// The folder of the saved report `id` under the bugs folder `root`. It must be a real folder
/// directly below `root` holding a `report.md`, including when a caller supplies a symlink.
pub fn report_folder(root: &Path, id: &str) -> Result<PathBuf, SessionError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(io_error("Invalid report ID."));
    }
    let folder = root.join(id);
    let valid = folder.symlink_metadata().is_ok_and(|meta| meta.is_dir())
        && folder.join("report.md").is_file();
    if !valid {
        return Err(io_error("The report was not found."));
    }
    Ok(folder)
}

/// Zips the saved report `id` under `root` into `<share>/<id>.zip` for a share sheet, after
/// deleting the files earlier shares left in `share`, and returns the zip's path. It blocks
/// while it writes.
pub fn share_archive(root: &Path, id: &str, share: &Path) -> Result<PathBuf, SessionError> {
    let folder = report_folder(root, id)?;
    let destination = share.join(format!("{id}.zip"));
    let write = || {
        std::fs::create_dir_all(share)?;
        for entry in std::fs::read_dir(share)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                std::fs::remove_file(entry.path())?;
            }
        }
        bugs::archive::save(&[folder], &destination)
    };
    write().map_err(io_error)?;
    Ok(destination)
}

fn io_error(error: impl std::fmt::Display) -> SessionError {
    SessionError::Io {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_only_accepts_a_saved_report_below_the_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let id = "20261113-102030-test";
        let folder = root.join(id);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("report.md"), "report").unwrap();
        assert_eq!(report_folder(root, id).unwrap(), folder);
        for bad in ["", "..", "../report", "/tmp/report", "x/y", "missing"] {
            assert!(report_folder(root, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_shared_report_replaces_the_last_share() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("bugs");
        let share = temp.path().join("share");
        for id in ["first", "second"] {
            std::fs::create_dir_all(root.join(id)).unwrap();
            std::fs::write(root.join(id).join("report.md"), id).unwrap();
        }
        let first = share_archive(&root, "first", &share).unwrap();
        assert!(first.is_file());
        let second = share_archive(&root, "second", &share).unwrap();
        assert_eq!(second, share.join("second.zip"));
        assert!(!first.exists(), "the earlier share was deleted");
        assert!(share_archive(&root, "missing", &share).is_err());
    }
}
