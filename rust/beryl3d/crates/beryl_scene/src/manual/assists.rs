//! What the manual table's assists show (docs/manual-multiplayer.md, "Assists"), each of which
//! the table answers through `Answer`, not a table action:
//!
//! - the proposal panel: the changes turn upkeep or combat suggests, each line with a checkbox,
//!   and Apply, which commits the checked lines as one undoable batch (the web's
//!   `ProposalPanel`, `ui/src/table/ProposalPanel.tsx`). The table marks each affected card
//!   itself, with a suggested mark in the view's script. On the desktop the panel stands in the
//!   log's column at the table's left edge; on a phone it is folded to a button under the log's icon;
//! - state nudges' labels: a small button on each card or life total a nudge is about, which
//!   applies the nudge's action (the web's `CardLabels` and `LifeLabels`,
//!   `ui/src/table/Labels.tsx`);
//! - the sorting tray, the prompt the table holds while a player scries, surveils or looks at
//!   their top cards: the cards not yet placed, then each pile's cards in order. A press on an
//!   unplaced card puts it at the end of the top pile; one on a placed card moves it to the end
//!   of the other pile, or to the front of the only one (`clickCard` in the web's
//!   `TablePromptView`, `ui/src/table/TablePromptView.tsx`). Done is enabled once every card is
//!   placed, and Cancel leaves the cards where they were.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{
    Answer, LabelView, ObjectId, PileName, ProposalView, Side, TablePrompt, TargetRef, ZoneName,
};
use beryl_protocol::script::Subject;
use glam::Vec2;

use super::TablePress;
use crate::element::{Align, Anchor, Element, Press, column, row, text};
use crate::input::Size;
use crate::inspect::viewer::{object_box, overlay};
use crate::screens::{checkbox, small};
use crate::table::Window;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{button, primary};

/// The proposal panel's name.
pub const PROPOSALS: &str = "Proposals";

/// The panel of `proposals`, or nothing while there are none: one section per proposal, its
/// title, a checkbox per line with whose change it is, and Apply, enabled while a line is
/// checked. On the desktop it stands in the log's column at the table's left edge, centred
/// between the two players' rows, where no card lies, since the space under the log holds the
/// opponent's lands. A phone's table has no room the panel could take without covering cards,
/// so there it is folded to a button under the log's icon button in the safe area's top right
/// corner, beside the opponent's battlefield, and opens under that button while `opened` is the
/// id of the first proposal, with the button at its top, as the folded log does. The web gives the panel a strip of its own over
/// the phone's hand instead.
#[must_use]
pub fn proposals(
    proposals: &[ProposalView],
    opened: Option<u32>,
    window: Window,
) -> Option<Element> {
    let first = proposals.first()?.id;
    let phone = window.phone();
    let mut panel = column()
        .name(PROPOSALS)
        .gap(space::M)
        .padding(space::M)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS);
    if !phone {
        panel = panel
            .children(proposals.iter().map(|each| proposal(each, false)))
            .width(crate::data::space::flat::table::log::WIDTH);
        let left = crate::data::space::flat::table::EDGE;
        panel.anchor = Some(Anchor::Point(
            Vec2::new(left, window.size.height / 2.0),
            Vec2::new(0.0, 0.5),
        ));
        return Some(panel);
    }
    let gutter = crate::data::space::flat::phone::GUTTER;
    let safe = window.safe_area;
    let right = window.size.width - safe.right - gutter;
    let top = safe.top + gutter + size::ICON_BUTTON + space::S;
    let corner = Anchor::Point(Vec2::new(right, top), Vec2::X);
    let open = opened == Some(first);
    let name = if open {
        "Hide proposals"
    } else {
        "Show proposals"
    };
    let mut heading = touch(small(button(PROPOSALS)), true).name(name);
    // Narrower than a button's own padding makes it, to keep clear of the opponent's cards.
    heading.style.padding = [0.0, space::M, 0.0, space::M];
    let heading = heading
        .selected(open)
        .on_press(Press::Table(TablePress::ToggleProposals(
            (!open).then_some(first),
        )));
    if !open {
        let mut folded = heading;
        folded.anchor = Some(corner);
        return Some(folded);
    }
    panel = panel
        .width(window.size.width - safe.left - safe.right - 2.0 * gutter)
        .align_items(Align::End)
        .child(heading)
        .children(
            proposals
                .iter()
                .map(|each| proposal(each, true).align_self(Align::Stretch)),
        );
    panel.anchor = Some(corner);
    Some(panel)
}

/// One proposal's section, its controls as tall as a touch needs on a `phone`.
fn proposal(proposal: &ProposalView, phone: bool) -> Element {
    let &ProposalView { id, revision, .. } = proposal;
    let lines = proposal.lines.iter().map(|line| {
        let whose = match line.whose {
            Side::You => "yours",
            Side::Opponent => "theirs",
        };
        let press = Press::Table(TablePress::ProposalLine {
            id,
            line: line.id,
            on: !line.on,
        });
        touch(
            checkbox(&format!("{} · {whose}", line.text), line.on, press),
            phone,
        )
    });
    let apply = touch(small(primary("Apply")), phone)
        .name("Apply")
        .align_self(Align::Start)
        .enabled(proposal.lines.iter().any(|line| line.on))
        .on_press(Press::Table(TablePress::ApplyProposal { id, revision }));
    column()
        .name(&proposal.title)
        .gap(space::S)
        .align_items(Align::Start)
        .child(
            text(&proposal.title)
                .style(type_style::BODY)
                .color(color::TEXT),
        )
        .children(lines)
        .child(apply)
}

/// The labels of state nudges: for each permanent and each life total a nudge is about, a row
/// of small buttons standing over its top edge, which follows the card as it moves. A nudge
/// about a card that is not on the battlefield shows nothing, as the web shows none.
#[must_use]
pub fn labels(labels: &[LabelView], view: &PlayerView, phone: bool) -> Vec<Element> {
    let mut on: Vec<(Subject, String, Vec<&LabelView>)> = Vec::new();
    for label in labels {
        let (subject, name) = match label.on {
            TargetRef::Object { slot } => {
                let battlefield = (view.objects.iter())
                    .any(|object| object.slot == slot && object.zone == ZoneName::Battlefield);
                if !battlefield {
                    continue;
                }
                (Subject::Object { slot }, format!("labels {}", slot.0))
            }
            TargetRef::Player { player } => {
                let whose = match player {
                    Side::You => "your",
                    Side::Opponent => "opponent's",
                };
                (Subject::Player { player }, format!("{whose} life labels"))
            }
            TargetRef::Ability { .. } => continue,
        };
        match on.iter_mut().find(|(each, ..)| *each == subject) {
            Some((.., group)) => group.push(label),
            None => on.push((subject, name, vec![label])),
        }
    }
    on.into_iter()
        .map(|(subject, name, group)| {
            let mut tags = row()
                .name(&name)
                .gap(space::S)
                .children(group.into_iter().map(|label| {
                    touch(small(button(&label.text)), phone)
                        .name(&label.text)
                        .on_press(Press::Table(TablePress::Label(label.id)))
                }));
            // On a phone the row straddles the card's top edge, reaching
            // `labels.phone_overlap` points into the card, so that both the card and the one
            // above it keep a touch's room uncovered; a row centred on the card would cover its
            // middle, and one wholly above it the lower half of the row above. The desktop's
            // cards are large enough for the row to stand centred on the card.
            tags.anchor = Some(if phone {
                let overlap = crate::data::space::flat::table::labels::PHONE_OVERLAP;
                Anchor::Above(subject, -overlap)
            } else {
                Anchor::Object(subject, Vec2::splat(0.5))
            });
            tags
        })
        .collect()
}

/// `element`, at least as tall as a touch needs on a `phone` (the "phone sizes" check).
fn touch(element: Element, phone: bool) -> Element {
    if phone {
        element.min_height(crate::data::audit::PHONE_TARGET_POINTS)
    } else {
        element
    }
}

/// The sorting tray of the table's prompt over a window of `window`'s size, if the prompt is a
/// sort: the cards not yet placed in a row, then a row for each pile with its name, and Done
/// and Cancel.
#[must_use]
pub fn sort_tray(view: &PlayerView, window: Size) -> Option<Element> {
    let Some(TablePrompt::Sort {
        title,
        cards,
        piles,
        placed,
    }) = view.table.as_ref()?.prompt.as_ref()
    else {
        return None;
    };
    let card =
        |slot: ObjectId| object_box(view, slot).on_press(Press::Table(TablePress::SortCard(slot)));
    let unplaced = (cards.iter().copied())
        .filter(|slot| !placed.iter().any(|pile| pile.contains(slot)))
        .map(card);
    let mut body = column()
        .name("sort")
        .gap(space::M)
        .align_items(Align::Center)
        .child(
            row()
                .name("unplaced")
                .gap(size::VIEWER_GAP)
                .children(unplaced),
        );
    for (pile, cards) in piles.iter().zip(placed) {
        let name = pile_name(*pile);
        body = body.child(
            column()
                .name(name)
                .gap(space::S)
                .align_items(Align::Center)
                .child(text(name).style(type_style::BODY).color(color::TEXT_DIM))
                .child(
                    row()
                        .name("cards")
                        .gap(size::VIEWER_GAP)
                        .children(cards.iter().copied().map(card)),
                ),
        );
    }
    let ready = placed.iter().map(Vec::len).sum::<usize>() == cards.len();
    let done = primary("Done")
        .name("done")
        .enabled(ready)
        .on_press(Press::Table(TablePress::SortDone));
    let cancel = button("Cancel")
        .name("close")
        .on_press(Press::Table(TablePress::SortCancel));
    Some(overlay(
        window,
        (title, None),
        None,
        body,
        vec![done, cancel],
    ))
}

/// The words a sorting tray shows over a pile, as the web's tray labels them.
fn pile_name(pile: PileName) -> &'static str {
    match pile {
        PileName::Top => "Top",
        PileName::Bottom => "Bottom",
        PileName::Graveyard => "Graveyard",
    }
}

/// What a press on `card` in the sorting tray of `view` answers (`clickCard` in the web's
/// `TablePromptView`): an unplaced card goes to the end of the top pile; a placed one to the
/// end of the other pile, or to the front of the only one. `None` when the view holds no sort,
/// or the card is not in it.
#[must_use]
pub fn sort_answer(view: &PlayerView, card: ObjectId) -> Option<Answer> {
    let Some(TablePrompt::Sort {
        cards,
        piles,
        placed,
        ..
    }) = view.table.as_ref()?.prompt.as_ref()
    else {
        return None;
    };
    if !cards.contains(&card) {
        return None;
    }
    let place = |row: usize, index: usize| {
        Some(Answer::Place {
            card,
            pile: *piles.get(row)?,
            index,
        })
    };
    match placed.iter().position(|pile| pile.contains(&card)) {
        None => {
            let top = piles.iter().position(|pile| *pile == PileName::Top)?;
            place(top, placed.get(top)?.len())
        }
        Some(row) if piles.len() == 1 => place(row, 0),
        Some(row) => {
            let other = 1 - row.min(1);
            place(other, placed.get(other)?.len())
        }
    }
}
