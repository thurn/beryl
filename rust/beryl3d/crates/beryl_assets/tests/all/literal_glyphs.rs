//! Every character a scene source file draws as a literal has a glyph in a bundled font. A
//! character with none draws nothing: the check mark U+2713 once left checked boxes empty.

use std::fs;
use std::path::{Path, PathBuf};

use kit_text::text::fonts::ALL;
use swash::FontRef;

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn scene_literals_have_a_glyph() {
    let fonts: Vec<FontRef> = ALL
        .iter()
        .map(|file| FontRef::from_index(file.data, 0).unwrap())
        .collect();
    let mut files = Vec::new();
    // Opening the folder through the helper makes its whole tree an input of this test;
    // `sources` then walks it.
    let root = beryl_test_files::root().unwrap();
    for crate_name in ["beryl_scene", "kit_ui"] {
        let directory = format!("rust/beryl3d/crates/{crate_name}/src");
        beryl_test_files::read_dir(&directory).unwrap();
        sources(&root.join(directory), &mut files);
    }
    let mut missing = Vec::new();
    for path in files {
        for (number, line) in fs::read_to_string(&path).unwrap().lines().enumerate() {
            // Comments may use any character; only code draws.
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for character in code.split("//").next().unwrap().chars() {
                if character.is_ascii() || character.is_whitespace() {
                    continue;
                }
                if !fonts.iter().any(|font| font.charmap().map(character) != 0) {
                    missing.push(format!("{}:{}: {character:?}", path.display(), number + 1));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "no bundled font has a glyph for:\n{}",
        missing.join("\n")
    );
}
