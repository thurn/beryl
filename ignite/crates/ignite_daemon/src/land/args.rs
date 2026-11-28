//! The arguments of `ignite land`.

/// The arguments of `ignite land`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LandArgs {
    /// The explicit note; `None` when the note is the landed commits' subjects.
    pub(super) note: Option<String>,
    pub(super) fresh: bool,
    /// Land even though the ticket holds regression checks the hub has switched off
    /// (`crate::claim::off_checks`).
    pub(super) allow_off: bool,
}

impl LandArgs {
    pub(super) fn parse(args: &[String]) -> Option<LandArgs> {
        let mut note = None;
        let mut fresh = false;
        let mut allow_off = false;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-m" | "--message" => note = Some(args.next()?.trim().to_owned()),
                "--fresh" => fresh = true,
                crate::claim::off_checks::ALLOW => allow_off = true,
                _ => return None,
            }
        }
        if note.as_ref().is_some_and(|n: &String| n.is_empty()) {
            return None;
        }
        Some(LandArgs {
            note,
            fresh,
            allow_off,
        })
    }
}
