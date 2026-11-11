//! A moment's scripts as text, for `specs-script`: what the session told each seat's screen
//! to show (plan K23), view by view, before anyone films it. A wrong script is a session bug
//! (the builder, `beryl_session::script`); a right script played wrongly is the stage's.

use std::fmt::Write as _;

use super::describe::{Unreached, events_line, trace, turn_line};
use super::{Moment, View, chosen};

/// `moment`'s scripts from `view`'s seat as text: each view shown once the setup was done,
/// played to the viewer's first live decision as `specs-describe` plays it, headed by its
/// number, turn and events, then its script one step a line (`beryl_session::script`'s text
/// form). The first view is shown at once: the stage plays nothing for it.
pub fn text(moment: &'static Moment, view: View) -> Result<String, Unreached> {
    let (_, shown) = trace(moment, view)?;
    let mut text = String::new();
    for (n, v) in shown.iter().enumerate() {
        let head = events_line(v).unwrap_or_else(|| format!("{}: no events", turn_line(v)));
        let at_once = if n == 0 { " (shown at once)" } else { "" };
        let _ = writeln!(text, "-- view {}{at_once}  {head}", n + 1);
        match &v.script {
            Some(script) if script.reset.is_some() || !script.beats.is_empty() => {
                for line in script.to_string().lines() {
                    let _ = writeln!(text, "  {line}");
                }
            }
            Some(_) => text.push_str("  (no steps)\n"),
            None => text.push_str("  (no script)\n"),
        }
    }
    Ok(text)
}

/// Prints the scripts of each moment of the specs for `cards` (by name or slug; all of them if
/// none), or of the one `moment` names, from each seat ([`text`]).
pub fn scripts(cards: &[String], moment: Option<&str>) -> Result<(), String> {
    let mut found = false;
    for spec in chosen(cards)? {
        for m in spec
            .moments()
            .iter()
            .filter(|m| moment.is_none_or(|n| n == m.name))
        {
            found = true;
            println!("== {} / {}  \"{}\"", spec.slug, m.name, m.title);
            for view in View::ALL {
                println!("{}:", view.name());
                print!("{}", text(m, view)?);
            }
            println!();
        }
    }
    if found {
        Ok(())
    } else {
        Err(format!(
            "no moment {cards:?} {}",
            moment.unwrap_or_default()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counterspell's first moment, from both seats: every view is numbered, the first shown
    /// at once, and the spell's flight to the stack is a step in each seat's script.
    #[test]
    fn a_moment_prints_each_seats_scripts_view_by_view() {
        let moment = &chosen(&["Counterspell".to_owned()]).unwrap()[0].moments()[0];
        for view in View::ALL {
            let text = text(moment, view).unwrap();
            assert!(text.starts_with("-- view 1 (shown at once)"), "{text}");
            assert!(text.contains("\n-- view 2 "), "{text}");
            assert!(
                text.lines()
                    .any(|l| l.contains("fly ") && l.contains("(Counterspell) hand>stack")),
                "{text}"
            );
        }
    }
}
