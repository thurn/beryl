//! Lists the distinct recording shapes and optionally replaces the test corpus.

use std::fmt::Write as _;

use super::options::{Options, Usage};
use super::{Printed, test};
use crate::corpus;

pub(super) fn corpus(options: &Options) -> Result<Printed, Usage> {
    if !options.rest.is_empty() {
        return Err(Usage("corpus takes no recording name".to_owned()));
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
