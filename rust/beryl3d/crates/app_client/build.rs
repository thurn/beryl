//! Embeds the story catalog. Authored tables become recordings here, keeping the protocol's
//! synthetic view builders out of the release client.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use game_probe::story::{Body, Capture, Story};

fn main() {
    let crates = Path::new("..");
    println!("cargo:rerun-if-changed={}", crates.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    let mut catalog = BTreeMap::new();
    for entry in fs::read_dir(crates).expect("workspace crates") {
        let entry = entry.expect("crate directory");
        let package = entry.file_name();
        let path = entry.path();
        if path.join("Cargo.toml").is_file() {
            collect(&package.to_string_lossy(), &path, None, &mut catalog);
        }
    }
    let mut includes = String::from("&[\n");
    for (index, (name, (path, feature))) in catalog.into_iter().enumerate() {
        let text =
            fs::read_to_string(&path).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let (text, plays) =
            compile(&name, text).unwrap_or_else(|why| panic!("{}: {why}", path.display()));
        let file = out.join(format!("{index}.story"));
        fs::write(&file, text).expect("write embedded story");
        let file = file.to_str().expect("the Cargo output path is UTF-8");
        let _ = writeln!(
            includes,
            "    ({name:?}, include_str!({file:?}), {plays}, {feature:?}),"
        );
    }
    includes.push_str("]\n");
    fs::write(out.join("stories.rs"), includes).expect("write story catalog");
}

fn collect(
    package: &str,
    path: &Path,
    stories: Option<&Path>,
    catalog: &mut BTreeMap<String, (PathBuf, Option<String>)>,
) {
    for entry in fs::read_dir(path).expect("story directory") {
        let entry = entry.expect("story entry");
        let kind = entry.file_type().expect("story file type");
        let path = entry.path();
        if kind.is_dir() && entry.file_name() != "target" {
            let root =
                stories.or_else(|| (entry.file_name() == "stories").then_some(path.as_path()));
            collect(package, &path, root, catalog);
        } else if let Some(root) = stories
            && kind.is_file()
            && path.extension().is_some_and(|ext| ext == "story")
        {
            let slug = path
                .file_stem()
                .and_then(|name| name.to_str())
                .expect("story name");
            assert!(
                slug.bytes()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-'),
                "invalid story name {slug}"
            );
            let name = format!("story/{package}/{slug}");
            let feature = path
                .parent()
                .and_then(|parent| parent.strip_prefix(root).ok())
                .filter(|relative| !relative.as_os_str().is_empty())
                .map(|relative| relative.to_string_lossy().into_owned())
                .or_else(|| {
                    let parent = root.parent()?.file_name()?.to_str()?;
                    (parent != package).then(|| parent.to_owned())
                });
            assert!(
                catalog.insert(name.clone(), (path, feature)).is_none(),
                "duplicate {name}"
            );
        }
    }
}

fn compile(name: &str, text: String) -> Result<(String, bool), String> {
    if !(text.starts_with("story table ") || text.starts_with("story feature ")) {
        return Ok((text, false));
    }
    let story = Story::parse(&text)?;
    let Body::Table(table) = story.body else {
        return Ok((text, false));
    };
    let plays = table.views.len() > 1 && table.actions.is_empty();
    let capture = Capture {
        source: name.to_owned(),
        at: 0,
        seat: beryl_protocol::protocol::Side::You,
        scale: 1.0,
        catch_up: false,
        messages: table.messages(),
        story_catalog: None,
        actions: table.actions,
    };
    capture
        .write(
            story.header,
            table.feature.as_deref(),
            story.profile.as_deref(),
        )
        .map(|text| (text, plays))
        .map_err(|why| why.to_string())
}
