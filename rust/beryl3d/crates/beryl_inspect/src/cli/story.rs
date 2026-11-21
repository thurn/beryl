//! Writes a portable story from the moment the replay driver actually drew.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use app_client::driver::{Duration, Moment};
use kit_probe::story::Header;
use serde_json::json;

use super::options::{Options, Usage, quote};
use super::{Printed, open_recording};
use crate::{scene::Scene, stories};

pub(super) fn from(options: &Options) -> Result<Printed, Usage> {
    if !options.rest.is_empty() || !options.actions.is_empty() {
        return Err(Usage(
            "write story from <recording> --at <moment> [--feature <id>]".to_owned(),
        ));
    }
    let page = open_recording(options)?;
    let mut scene = Scene::Recording(Box::new(page));
    let time = super::time_of(&mut scene, options, &options.at)?;
    scene
        .seek(&Moment::At(Duration(time)))
        .map_err(|error| Usage(error.to_string()))?;
    let Scene::Recording(mut page) = scene else {
        unreachable!("the command opens a recording");
    };
    let header = Header {
        layout: options.layout,
        window: options.size,
    };
    let feature = options.value("--feature");
    let profile = options.value(super::options::PROFILE);
    let text = match feature {
        Some("settings") => page.settings_story().write(header, profile),
        Some(id) if page.feature_element(id).is_none() => {
            return Err(Usage(format!(
                "feature {id:?} has no standalone view at this moment"
            )));
        }
        _ => page
            .capture()
            .write(header, feature, profile)
            .map_err(|error| Usage(error.to_string()))?,
    };
    let path = write_story(options.value("--out").map(Path::new), text.as_bytes())?;
    let path = path.to_string_lossy();
    let replay = format!("just b3d tree {}", quote(&path));
    let out = if options.json {
        format!("{}\n", json!({ "path": path, "replay": replay }))
    } else {
        format!("{path}\n{replay}\n")
    };
    Ok(Printed::text(out))
}

fn write_story(path: Option<&Path>, text: &[u8]) -> Result<PathBuf, Usage> {
    static NUMBER: AtomicU32 = AtomicU32::new(0);

    if let Some(path) = path {
        if !stories::has_extension(path) {
            return Err(Usage("--out needs a file name ending in .story".to_owned()));
        }
        write_new(path, text).map_err(|error| Usage(format!("{}: {error}", path.display())))?;
        return Ok(path.to_owned());
    }

    let directory = std::env::temp_dir();
    loop {
        let index = NUMBER.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("beryl-{}-{index}.story", std::process::id()));
        match write_new(&path, text) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(Usage(format!("{}: {error}", path.display()))),
            Ok(()) => return Ok(path),
        }
    }
}

fn write_new(path: &Path, text: &[u8]) -> io::Result<()> {
    let mut file = std::fs::File::create_new(path)?;
    if let Err(error) = file.write_all(text) {
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(())
}
