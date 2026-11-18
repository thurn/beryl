//! The `finds` page: what each exploration family found and spent over the last 30 days, its
//! share of the spend, and each rebuild count's series.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::time::Timestamp;

use super::attention::frame;
use super::components::{self as c, PageOptions, esc};
use super::data::ReadError;

/// The days of exploration the page adds up.
const DAYS: f64 = 30.0;

/// How many measurements of a rebuild count the page lists.
const SERIES: usize = 10;

/// The `finds` page.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
) -> Result<PageOptions, ReadError> {
    let since = now.plus_seconds(-DAYS * 86_400.0).day();
    let mut statement = conn.prepare(
        "SELECT family, SUM(bugs), SUM(instructions) FROM explore WHERE day >= ?1
         GROUP BY family ORDER BY family",
    )?;
    let families = statement
        .query_map([&since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let total: f64 = families.iter().map(|family| family.2).sum();
    let explored = if families.is_empty() {
        c::empty("No exploration has run.")
    } else {
        c::rows(families.iter().map(|(family, bugs, instructions)| {
            let share = if total > 0.0 {
                100.0 * instructions / total
            } else {
                0.0
            };
            c::row(
                &(esc(family) + &c::muted(&format!(" · {bugs} bugs"))),
                &format!(
                    "{:.0} billion instructions, {share:.0}%",
                    instructions / 1e9
                ),
                None,
            )
        }))
    };

    let mut names = conn.prepare("SELECT DISTINCT name FROM rebuilds ORDER BY name")?;
    let names = names
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    // A rebuild count stores no time, so the series is ordered by when the hub first saw the
    // commit as a tip, and then by commit for commits that were never tips.
    let mut series = conn.prepare(
        "SELECT r.instructions FROM rebuilds r LEFT JOIN tips t ON t.commit_sha = r.commit_sha
         WHERE r.name = ?1 ORDER BY COALESCE(t.seen, '') DESC, r.commit_sha DESC LIMIT ?2",
    )?;
    let mut rebuilds = Vec::new();
    for name in &names {
        let mut values = series
            .query_map(rusqlite::params![name, SERIES as i64], |row| {
                row.get::<_, f64>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        values.reverse();
        let shown: Vec<String> = values
            .iter()
            .map(|value| format!("{:.0}", value / 1e9))
            .collect();
        rebuilds.push(c::row(
            &esc(name),
            &format!("{} billion", shown.join(", ")),
            None,
        ));
    }
    let rebuilt = if rebuilds.is_empty() {
        c::empty("No rebuild has been counted.")
    } else {
        c::rows(rebuilds)
    };
    let body = c::section(
        "Exploration, last 30 days",
        &explored,
        Some(families.len()),
        None,
        3,
    ) + &c::section(
        "Rebuild counts, oldest to newest",
        &rebuilt,
        Some(names.len()),
        None,
        3,
    );
    Ok(frame(
        root,
        seq,
        "finds",
        "What exploration found, and the rebuild counts.",
        &body,
    ))
}
