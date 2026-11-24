use beryl_protocol::protocol::ZoneName;
use game_table::table::{PileShown, pile_name};
use kit_ui::element::Element;
use kit_ui::widgets::tip_label;

/// The count of the pile at `index`, as its tip says it: the zone and the number, "Library ·
/// 33" (`pileTip` in `ui/src/game/Piles.tsx`). It is named for the pile and the number, as
/// "Your library 60", so that the tree's line for it says both.
pub(super) fn count_text(index: usize, count: usize) -> Element {
    let (side, zone) = PileShown::of_index(index);
    let words = match zone {
        ZoneName::Library => "Library",
        ZoneName::Graveyard => "Graveyard",
        _ => "Exile",
    };
    // It lets the pointer through to the pile and any card over it, as `.tip` takes no
    // pointer events.
    let mut element = tip_label(format!("{words} \u{b7} {count}")).passes(true);
    element.name = format!("{} {count}", pile_name(side, zone));
    element
}
