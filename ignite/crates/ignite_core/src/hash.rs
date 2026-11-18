//! Input hashes: SHA-256 over a check's sorted inputs, its family's commands, the toolchain
//! version and, for a check that is not portable, the operating system.
//!
//! Two trees that give a check the same hash must give it the same result, so the hash covers
//! everything the check reads. Git already stores a content hash for every file and directory,
//! so each input contributes its path and git object ID rather than its contents. Every field is
//! written with its length in front, so no two different sets of parts can run together into
//! the same bytes.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::config::Os;
use crate::ids::{InputHash, Oid};
use crate::manifest::{Family, Manifest, Platforms};

/// Written first, so that a change to the layout below gives every check a new hash.
const LAYOUT: &str = "ignite input hash 1";

/// Everything that goes into one check's input hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HashParts<'a> {
    /// Each input path with its git blob or tree ID.
    pub inputs: &'a [(String, Oid)],
    /// The family's `list` and `run` commands and its products' build commands.
    pub commands: &'a [String],
    pub toolchain: &'a str,
    /// `None` for a check with `platforms = "any"`.
    pub platform: Option<Os>,
}

/// The input hash of one check. The order of `inputs` does not matter; the order of `commands`
/// does, because [`commands`] gives them in a fixed order.
pub fn input_hash(parts: &HashParts<'_>) -> InputHash {
    let mut inputs: Vec<&(String, Oid)> = parts.inputs.iter().collect();
    inputs.sort();
    inputs.dedup();
    let mut hasher = Sha256::new();
    field(&mut hasher, LAYOUT);
    count(&mut hasher, inputs.len());
    for (path, oid) in inputs {
        field(&mut hasher, path);
        field(&mut hasher, oid.as_str());
    }
    count(&mut hasher, parts.commands.len());
    for command in parts.commands {
        field(&mut hasher, command);
    }
    field(&mut hasher, parts.toolchain);
    field(&mut hasher, parts.platform.map_or("", Os::as_str));
    let hex = hasher
        .finalize()
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    InputHash::new(hex).unwrap_or_else(|_| unreachable!("SHA-256 gives 64 hex digits"))
}

/// The operating system a check's hash names: none for a family whose checks pass anywhere,
/// and the machine's own otherwise.
pub fn platform(platforms: &Platforms, os: Os) -> Option<Os> {
    if platforms.is_any() { None } else { Some(os) }
}

/// The commands a family's hash covers, in a fixed order: `list`, `run`, then the build
/// command of each product it needs, in the order of `needs`.
pub fn commands(family: &Family, manifest: &Manifest) -> Vec<String> {
    let mut commands: Vec<String> = [&family.list, &family.run]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    commands.extend(
        family
            .needs
            .iter()
            .filter_map(|name| manifest.products.get(name))
            .map(|product| product.build.clone()),
    );
    commands
}

fn count(hasher: &mut Sha256, n: usize) {
    hasher.update((n as u64).to_le_bytes());
}

fn field(hasher: &mut Sha256, text: &str) {
    count(hasher, text.len());
    hasher.update(text.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::AnyPlatform;

    fn oid(c: char) -> Oid {
        Oid::new(c.to_string().repeat(40)).unwrap()
    }

    fn hash(inputs: &[(&str, char)], platform: Option<Os>) -> InputHash {
        let inputs: Vec<(String, Oid)> = inputs
            .iter()
            .map(|(p, c)| ((*p).to_owned(), oid(*c)))
            .collect();
        let commands = ["cargo nextest run".to_owned()];
        input_hash(&HashParts {
            inputs: &inputs,
            commands: &commands,
            toolchain: "1.93.0",
            platform,
        })
    }

    #[test]
    fn the_order_of_inputs_does_not_matter() {
        assert_eq!(
            hash(&[("a", '1'), ("b", '2')], None),
            hash(&[("b", '2'), ("a", '1')], None)
        );
    }

    #[test]
    fn a_changed_object_id_changes_the_hash() {
        let base = hash(&[("a", '1'), ("b", '2')], None);
        assert_ne!(base, hash(&[("a", '1'), ("b", '3')], None));
        assert_ne!(base, hash(&[("a", '4'), ("b", '2')], None));
    }

    #[test]
    fn fields_cannot_run_together() {
        assert_ne!(hash(&[("ab", '1')], None), hash(&[("a", '1')], None));
        let inputs = [("a".to_owned(), oid('1'))];
        let split = |commands: &[&str]| {
            let commands: Vec<String> = commands.iter().map(|c| (*c).to_owned()).collect();
            input_hash(&HashParts {
                inputs: &inputs,
                commands: &commands,
                toolchain: "t",
                platform: None,
            })
        };
        assert_ne!(split(&["a b"]), split(&["a", "b"]));
        assert_ne!(split(&["a"]), split(&["b"]));
    }

    #[test]
    fn the_platform_enters_only_for_a_family_that_is_not_portable() {
        let any = Platforms::Any(AnyPlatform::Any);
        let each = Platforms::Each(vec![Os::Macos]);
        assert_eq!(platform(&any, Os::Macos), None);
        assert_eq!(platform(&each, Os::Macos), Some(Os::Macos));
        let inputs = [("a", '1')];
        assert_eq!(
            hash(&inputs, platform(&any, Os::Macos)),
            hash(&inputs, None)
        );
        assert_ne!(
            hash(&inputs, platform(&any, Os::Macos)),
            hash(&inputs, platform(&each, Os::Macos))
        );
    }

    #[test]
    fn commands_follow_list_run_then_needs() {
        let text = r#"
[family.f]
kind = "regression"
adapter = "jsonl"
platforms = "any"
needs = ["b", "a"]
list = "list"
run = "run {checks}"
inputs = ["x"]

[product.a]
build = "build a"
inputs = ["x"]
outputs = ["out"]

[product.b]
build = "build b"
inputs = ["x"]
outputs = ["out"]
"#;
        let manifest = Manifest::from_files(&[("f.toml".into(), text.to_owned())]).unwrap();
        let family = &manifest.families["f"].family;
        assert_eq!(
            commands(family, &manifest),
            ["list", "run {checks}", "build b", "build a"]
        );
    }
}
