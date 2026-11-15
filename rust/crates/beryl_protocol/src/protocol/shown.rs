//! When two views are equal.

use super::PlayerView;

/// Two views are equal when they show the same: a view's script is how one screen came to it,
/// not part of it. A game and its replay end on equal views, the replay's reached with no
/// view before it.
impl PartialEq for PlayerView {
    fn eq(&self, other: &PlayerView) -> bool {
        shown(self) == shown(other)
    }
}

/// Everything of `view` but its script: a field added to a view is named here, or this does
/// not compile.
fn shown(view: &PlayerView) -> impl PartialEq + '_ {
    let PlayerView {
        status,
        handling,
        pending_attackers,
        turn,
        players,
        battlefield,
        objects,
        stack,
        stops,
        events,
        undone,
        script: _,
        table,
    } = view;
    (
        status,
        handling,
        pending_attackers,
        turn,
        players,
        battlefield,
        objects,
        stack,
        stops,
        events,
        undone,
        table,
    )
}
