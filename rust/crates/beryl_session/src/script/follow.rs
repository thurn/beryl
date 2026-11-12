//! A seat's screen followed from the views it was sent: what a stage that played every one of
//! their scripts holds. A page that shows a view other than the seat's first at once (a spec
//! moment's first view after its setup) starts over from it with [`Follow::restart`], so the
//! scripts after it name what its stage holds: the abilities on the stack by the numbers the
//! scripts before gave them, and the spans they left open.

use super::check::carried;
use super::replay::replay;
use super::{Open, Reset, Script, Table, spans};
use crate::protocol::{PlayerView, StackItem};

/// The table and the open spans a seat's scripts leave its screen with.
#[derive(Clone, Debug, Default)]
pub struct Follow {
    /// None before a script that resets, and after one that cannot be played.
    shown: Option<(Table, Vec<Open>)>,
}

impl Follow {
    /// Plays the script `view` came with over what the views before it left.
    ///
    /// # Errors
    ///
    /// When the script cannot be played there or breaks a rule invariant, or does not reset
    /// with nothing played before it. Nothing is then followed until a script that resets.
    pub fn play(&mut self, view: &PlayerView) -> Result<(), String> {
        let shown = self.shown.take();
        let script = view.script.as_ref().ok_or("the view has no script")?;
        let (table, open) = match (&script.reset, shown) {
            (Some(_), _) => (Table::of(view), Vec::new()),
            (None, Some(shown)) => shown,
            (None, None) => return Err("no script that resets came before the view".to_owned()),
        };
        let open = carried(table.clone(), &open, script)?;
        self.shown = Some((replay(table, script)?, open));
        Ok(())
    }

    /// The script that starts a stage over at the view played last, in place of the one it
    /// came with: a reset that numbers the abilities on its stack as the scripts so far have,
    /// then the spans they left open, begun at once. An error says why there is none: nothing
    /// is followed, or the view's own table is not the one its scripts leave, which a stage
    /// starting over from the view would then hold.
    ///
    /// # Errors
    ///
    /// When nothing is followed, or `view`'s table is not the one followed.
    pub fn restart(&self, view: &PlayerView) -> Result<Script, String> {
        let (table, open) = self
            .shown
            .as_ref()
            .ok_or("no script that resets came before the view")?;
        let own = Table::of(view);
        if !table.shows(&own) {
            return Err(format!(
                "the view's table is not the one its scripts leave: {}",
                super::replay::difference(table, &own)
            ));
        }
        let abilities = table
            .stack
            .iter()
            .filter(|e| matches!(e.item, StackItem::Ability { .. }))
            .filter_map(|e| e.id)
            .collect();
        Ok(Script {
            reset: Some(Reset { abilities }),
            beats: spans::shown(open),
        })
    }
}
