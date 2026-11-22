use super::{areas, named_files, resolve_named_file, resolved_files};

#[test]
fn a_short_path_gets_the_area_of_the_file_it_resolves_to() {
    let known = ["rust/crates/scene/src/a.rs".to_owned()];
    let short = ["scene/src/a.rs".to_owned()];
    assert_eq!(areas(&resolved_files(&short, &known)), areas(&known));
    assert_ne!(areas(&short), areas(&known));
}

#[test]
fn a_ticket_names_the_paths_on_its_files_line() {
    let text = "Fix the parser.\n\n\
                Files: game_scene/src/element/style.rs, `tools/b.py` (the parser), \
                docs/a.md, `e/f.rs`.\n\
                - **Files:** ui/src/x.ts; the `why` command in ignite/crates/ignite_cli\n\
                Not a files line: c/d.rs";
    assert_eq!(
        named_files(text),
        vec![
            "tools/b.py",
            "e/f.rs",
            "game_scene/src/element/style.rs",
            "docs/a.md",
            "ui/src/x.ts",
        ]
    );
}

#[test]
fn a_short_path_resolves_to_the_one_tracked_file_it_ends() {
    let tracked: Vec<String> = [
        "rust/client/crates/game_scene/src/style.rs",
        "rust/a/src/lib.rs",
        "rust/b/src/lib.rs",
        "docs/a.md",
    ]
    .map(str::to_owned)
    .to_vec();
    let resolve = |path: &str| resolve_named_file(path, &tracked);
    assert_eq!(resolve("docs/a.md").as_deref(), Some("docs/a.md"));
    assert_eq!(
        resolve("game_scene/src/style.rs").as_deref(),
        Some("rust/client/crates/game_scene/src/style.rs")
    );
    // Two crates hold a `src/lib.rs`, and nothing holds `scene/src/style.rs` as a whole part.
    assert_eq!(resolve("src/lib.rs"), None);
    assert_eq!(resolve("scene/src/style.rs"), None);
    assert_eq!(resolve("docs/missing.md"), None);
}

#[test]
fn grouped_paths_expand_with_their_shared_prefix_and_suffix() {
    assert_eq!(
        named_files(
            "Start with `src/{first.rs, nested/second.rs}`.
             Files: `tools/{first, second}.py`, src/first.rs, docs/guide.md"
        ),
        [
            "src/first.rs",
            "src/nested/second.rs",
            "tools/first.py",
            "tools/second.py",
            "docs/guide.md",
        ]
    );
}
