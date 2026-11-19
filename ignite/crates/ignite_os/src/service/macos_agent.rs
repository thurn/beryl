//! Reading back what an agent's property list runs, so that one written by an older build can be
//! written again with this build's settings and nothing else changed ([`super::refreshed`]).
//!
//! The file is the one [`super::plist`] wrote, so this reads only that layout: one element per
//! line. A file it cannot read whole, such as one a person edited with another layout, reads
//! as `None` and is left alone.

/// What an agent runs, as its property list records it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Recorded {
    pub program: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub log: String,
}

/// The program, environment and log file of the property list `text`, in the order it lists
/// them, or `None` when any of the three is missing or not in [`super::plist`]'s layout.
pub(super) fn read(text: &str) -> Option<Recorded> {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let after = |key: &str| {
        let wanted = format!("<key>{key}</key>");
        lines
            .iter()
            .position(|line| *line == wanted)
            .map(|at| &lines[at + 1..])
    };

    let mut program = Vec::new();
    let rest = after("ProgramArguments")?;
    if rest.first() != Some(&"<array>") {
        return None;
    }
    for line in &rest[1..] {
        if *line == "</array>" {
            break;
        }
        program.push(element(line, "string")?);
    }

    let mut environment = Vec::new();
    let rest = after("EnvironmentVariables")?;
    if rest.first() != Some(&"<dict>") {
        return None;
    }
    let mut pairs = rest[1..].iter();
    loop {
        let line = pairs.next()?;
        if *line == "</dict>" {
            break;
        }
        let name = element(line, "key")?;
        let value = element(pairs.next()?, "string")?;
        environment.push((name, value));
    }

    let log = element(after("StandardOutPath")?.first()?, "string")?;
    Some(Recorded {
        program,
        environment,
        log,
    })
}

/// The text of `<tag>text</tag>` on one line, with [`super::escape`]'s escapes undone.
fn element(line: &str, tag: &str) -> Option<String> {
    let inner = line
        .strip_prefix(&format!("<{tag}>"))?
        .strip_suffix(&format!("</{tag}>"))?;
    Some(
        inner
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}
