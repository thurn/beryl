//! Lists the distinct recording shapes and optionally replaces the test corpus.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use app_client::driver::Mode;
use serde_json::json;

use super::options::{Options, Usage};
use super::{Printed, test};
use crate::{corpus, pages, recordings, runner};

pub(super) const SNAPSHOT: &str = "--snapshot";
pub(super) const AGAINST: &str = "--against";

pub(super) fn corpus(options: &Options) -> Result<Printed, Usage> {
    if !options.rest.is_empty() {
        return Err(Usage("corpus takes no recording name".to_owned()));
    }
    if let Some(path) = options.value(SNAPSHOT) {
        return snapshot(Path::new(path)).map_err(Usage);
    }
    if let Some(path) = options.value(AGAINST) {
        return sweep(Path::new(path)).map_err(Usage);
    }
    let groups = corpus::representatives().map_err(Usage)?;
    let recordings: usize = groups.iter().map(|group| group.count).sum();
    let mut out = String::new();
    for group in &groups {
        let _ = writeln!(out, "{}\t{}", group.name, group.count);
    }
    let _ = writeln!(
        out,
        "{} signatures across {recordings} recording names",
        groups.len()
    );
    if options.switch(test::UPDATE) {
        let names = corpus::update(&groups).map_err(Usage)?;
        let _ = writeln!(out, "Wrote {names} names to data/beryl3d/corpus.txt");
    }
    Ok(Printed::text(out))
}

/// Writes the corpus signatures once for all the parts of a sweep.
fn snapshot(path: &Path) -> Result<Printed, String> {
    corpus::read()?;
    let shapes: HashSet<_> = corpus::shapes()?
        .into_iter()
        .map(|(_, shape)| shape)
        .collect::<Result<_, _>>()?;
    let bytes = postcard::to_stdvec(&shapes).map_err(|error| error.to_string())?;
    let compressed = miniz_oxide::deflate::compress_to_vec(&bytes, 1);
    fs::write(path, compressed).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Printed::text(String::new()))
}

fn sweep(path: &Path) -> Result<Printed, String> {
    let compressed = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let bytes = miniz_oxide::inflate::decompress_to_vec(&compressed)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let baseline: HashSet<corpus::Shape> =
        postcard::from_bytes(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut missing = Vec::new();
    let mut faults = Vec::new();
    for (name, shape) in corpus::shapes()? {
        match shape {
            Ok(shape) if !baseline.contains(&shape) => missing.push(name),
            Ok(_) => {}
            Err(error) => faults.push(error),
        }
    }
    let windows = pages::Windows::All.windows();
    let tests: Vec<_> = recordings::entries()?
        .iter()
        .flat_map(|entry| {
            windows.iter().map(|window| runner::Test {
                recording: entry.name.to_owned(),
                seat: super::options::side_of(entry.seat()),
                size: window.size,
                scale: window.scale,
                profile: None,
            })
        })
        .collect();
    for outcome in runner::run(&tests, Mode::EveryTick) {
        if let Some(error) = outcome.error {
            faults.push(format!("{}: {error}", outcome.test.name()));
        }
        if let Some(first) = outcome.faults.first() {
            faults.push(format!(
                "{}: {} fault(s), first {} at {} ms: {}",
                outcome.test.name(),
                outcome.faults.len(),
                first.rule.word(),
                first.at / 1_000_000,
                first.message
            ));
        }
    }
    let fault = !missing.is_empty() || !faults.is_empty();
    let text = format!(
        "{}\n",
        json!({ "missing": missing, "faults": faults, "tests": tests.len() })
    );
    Ok(Printed { text, fault })
}
