//! Version 1 report bundles contain a manifest and the known evidence files for each report.
//! The format is independent of the file picker or any future transport.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde_json::json;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const FILES: &[&str] = &[
    "report.md",
    "screenshot.png",
    "window.png",
    "replay.json",
    "searches.json",
    "view.json",
    "fixture.json",
    "ui.json",
    "journal.jsonl",
];

/// Completed report folders at the root, including reports already filed as beads.
/// Archived reports in `done/` are excluded.
pub fn active(root: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut folders = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        // Ignore hidden staging directories and names outside the report-ID alphabet.
        if entry.file_type()?.is_dir()
            && name != "done"
            && name
                .as_encoded_bytes()
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || *c == b'-')
            && entry.path().join("report.md").is_file()
        {
            folders.push(entry.path());
        }
    }
    folders.sort_unstable();
    Ok(folders)
}

/// Writes an archive atomically at the chosen destination.
pub fn save(folders: &[PathBuf], destination: &Path) -> io::Result<()> {
    let directory = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing destination directory"))?;
    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
    let mut zip = ZipWriter::new(temp.as_file_mut());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut ids = Vec::new();
    for folder in folders {
        let id = folder
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("invalid report ID"))?;
        for name in FILES {
            let path = folder.join(name);
            match path.symlink_metadata() {
                Ok(meta) if meta.is_file() => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound && *name != "report.md" => {
                    continue;
                }
                Err(error) => return Err(error),
                Ok(_) => return Err(io::Error::other(format!("invalid report file: {name}"))),
            }
            zip.start_file(format!("reports/{id}/{name}"), options)?;
            io::copy(&mut File::open(path)?, &mut zip)?;
        }
        ids.push(id);
    }
    zip.start_file("manifest.json", options)?;
    zip.write_all(&serde_json::to_vec(&json!({
        "format": "beryl-bug-reports",
        "version": 1,
        "reports": ids,
    }))?)?;
    zip.finish()?;
    temp.persist(destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_only_completed_active_reports_and_known_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("bugs");
        for id in ["new-report", "done/old-report", ".tmp-incomplete"] {
            let folder = root.join(id);
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("report.md"), "description").unwrap();
            fs::write(folder.join("replay.json"), "{}").unwrap();
            fs::write(folder.join("other.txt"), "private").unwrap();
        }
        let destination = temp.path().join("reports.zip");
        save(&active(&root).unwrap(), &destination).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(destination).unwrap()).unwrap();
        assert_eq!(zip.len(), 3);
        let manifest: serde_json::Value =
            serde_json::from_reader(zip.by_name("manifest.json").unwrap()).unwrap();
        assert_eq!(manifest["reports"], json!(["new-report"]));
        assert!(zip.by_name("reports/new-report/replay.json").is_ok());
    }
}
