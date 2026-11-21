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

use crate::features::proposals::Message as ProposalsMessage;
use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{
    Answer, LabelView, ObjectId, PileName, ProposalView, Side, TablePrompt, TargetRef, ZoneName,
};
use beryl_protocol::script::Subject;
use game_hud::hud::placement_scene::PlacementScene;
use kit_base::data::Card;

use glam::Vec2;

use crate::features::table::Message as TableMessage;
use game_hud::inspect::viewer::object_box;
use game_table::profiles::Profile;
use game_table::table::Window;
use kit_base::tokens::{color, size, space, type_style};
use kit_ui::element::{Align, Anchor, Element, column, row, text};
use kit_ui::input::Size;
use kit_ui::widgets::screen::overlay;
use kit_ui::widgets::start_panel::{checkbox, small};
use kit_ui::widgets::{button, primary};

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
    profile: Profile<'_>,
    anchor: Option<Anchor>,
) -> Option<Element> {
    let first = proposals.first()?.id;
    let mut panel = column()
        .name(PROPOSALS)
        .gap(space::M)
        .padding(space::M)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS);
    let Some(slot) = profile.slot("manual.proposals") else {
        panel = panel
            .children(proposals.iter().map(|each| proposal(each, None)))
            .width(kit_base::data::space::flat::table::log::WIDTH);
        let left = kit_base::data::space::flat::table::EDGE;
        panel.anchor = Some(Anchor::Point(
            Vec2::new(left, window.size.height / 2.0),
            Vec2::new(0.0, 0.5),
        ));
        return Some(panel);
    };
    let rect = slot.rect;
    let target = profile.data().checks.target_points;
    let corner = Anchor::Point(Vec2::new(rect.x + rect.width, rect.y), Vec2::X);
    let open = opened == Some(first);
    let name = if open {
        "Hide proposals"
    } else {
        "Show proposals"
    };
    let mut heading = small(button(PROPOSALS)).min_height(target).name(name);
    // Narrower than a button's own padding makes it, to keep clear of the opponent's cards.
    heading.style.padding = [0.0, space::M, 0.0, space::M];
    let heading = heading
        .selected(open)
        .on_press(ProposalsMessage::Toggle((!open).then_some(first)));
    if !open {
        let mut folded = heading;
        folded.anchor = Some(corner);
        return Some(folded);
    }
    panel = panel
        .width(rect.width)
        .align_items(Align::End)
        .child(heading)
        .children(
            proposals
                .iter()
                .map(|each| proposal(each, Some(target)).align_self(Align::Stretch)),
        );
    panel.anchor = anchor.or(Some(corner));
    Some(panel)
}

/// Places an open phone panel and records its clearance from the table.
#[must_use]
pub fn proposal_anchor(
    proposals: &[ProposalView],
    opened: Option<u32>,
    profile: Profile<'_>,
    layout: Option<&PlacementScene>,
) -> Option<Anchor> {
    let first = proposals.first()?;
    if opened != Some(first.id) {
        return None;
    }
    let rect = profile.slot("manual.proposals")?.rect;
    super::profiles::proposal_slot(
        proposals,
        rect,
        profile.bands()?,
        layout,
        profile.data().checks.target_points,
    )
}

/// One proposal's section, its controls as tall as a touch needs on a `phone`.
fn proposal(proposal: &ProposalView, target: Option<f32>) -> Element {
    let &ProposalView { id, revision, .. } = proposal;
    let lines = proposal.lines.iter().map(|line| {
        let whose = match line.whose {
            Side::You => "yours",
            Side::Opponent => "theirs",
        };
        let press = ProposalsMessage::Line {
            id,
            line: line.id,
            on: !line.on,
        };
        touch(
            checkbox(&format!("{} · {whose}", line.text), line.on).on_press(press),
            target,
        )
    });
    let apply = touch(small(primary("Apply")), target)
        .name("Apply")
        .align_self(Align::Start)
        .enabled(proposal.lines.iter().any(|line| line.on))
        .on_press(ProposalsMessage::Apply { id, revision });
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
pub fn labels(labels: &[LabelView], view: &PlayerView, profile: Profile<'_>) -> Vec<Element> {
    let manual = profile.data().manual;
    let target = manual.map(|_| profile.data().checks.target_points);
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
                    touch(small(button(&label.text)), target)
                        .name(&label.text)
                        .on_press(TableMessage::Label(label.id))
                }));
            // In the compact layout the label straddles the card's top edge. This leaves
            // room to press both the card and the one above it. Larger cards have room
            // for the label in the middle.
            tags.anchor = Some(if let Some(manual) = manual {
                Anchor::Above(subject, -manual.label_overlap)
            } else {
                Anchor::Object(subject, Vec2::splat(0.5))
            });
            tags
        })
        .collect()
}

/// `element`, at least as tall as a touch needs on a `phone` (the "phone sizes" check).
fn touch(element: Element, target: Option<f32>) -> Element {
    if let Some(target) = target {
        element.min_height(target)
    } else {
        element
    }
}

/// The sorting tray of the table's prompt over a window of `window`'s size, if the prompt is a
/// sort: the cards not yet placed in a row, then a row for each pile with its name, and Done
/// and Cancel.
#[must_use]
pub fn sort_tray(view: &PlayerView, window: Size, shape: Card) -> Option<Element> {
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
        |slot: ObjectId| object_box(view, slot, shape).on_press(TableMessage::SortCard(slot));
    let unplaced = (cards.iter().copied())
        .filter(|slot| !placed.iter().any(|pile| pile.contains(slot)))
        .map(card);
    let mut body = column()
        .name("sort")
        .gap(space::M)
        .align_items(Align::Center)
        .max_height(super::order::tray_height(window))
        .overflow(kit_ui::element::Overflow::ScrollY)
        .child(super::order::tray_grid(
            "unplaced",
            unplaced.collect(),
            window,
        ));
    for (pile, cards) in piles.iter().zip(placed) {
        let name = pile_name(*pile);
        body = body.child(
            column()
                .name(name)
                .gap(space::S)
                .align_items(Align::Center)
                .child(text(name).style(type_style::BODY).color(color::TEXT_DIM))
                .child(super::order::tray_grid(
                    "cards",
                    cards.iter().copied().map(card).collect(),
                    window,
                )),
        );
    }
    let ready = placed.iter().map(Vec::len).sum::<usize>() == cards.len();
    let done = primary("Done")
        .name("done")
        .enabled(ready)
        .on_press(TableMessage::SortDone);
    let cancel = button("Cancel")
        .name("close")
        .on_press(TableMessage::SortCancel);
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
