//! Where the cards the opponent is handling rest while they look at them (`lift`): the top cards
//! of a library, lying face down beside it as Arena's do, or a hand, held up in front of the
//! camera in a small fan as the web interface's `handlingPlacements` (`ui/src/game/layout.ts`)
//! holds them. A card of a hand shows the face its record shows to the viewer.
//!
//! The handling is the view's (`PlayerView::handling`), or, while a script plays, the one its
//! `lift` steps so far have begun. Which cards it holds follows the web's `handledAt`
//! (`ui/src/stage/table.ts`).

use beryl_protocol::protocol::{CardHandling, ObjectId, ObjectView, PlayerView, Side, ZoneName};
use beryl_protocol::script::{Act, Span, Table as Played};
use glam::{Quat, Vec2};

use super::place::{Group, Placement, Zones, members};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, LAYER, Layout};
use crate::data::space::verbs::lift;
use crate::frame::Place;
use crate::pose::Pose;

/// The group that holds the cards `handling` takes, if the table places any of them.
#[must_use]
pub(crate) fn group_of(handling: CardHandling) -> Option<Group> {
    match handling {
        CardHandling::Library { side, .. }
        | CardHandling::Pile {
            side,
            pile: ZoneName::Library,
            ..
        } => Some(Group::Pile {
            side,
            zone: ZoneName::Library,
        }),
        CardHandling::Hand { side: Side::You } => Some(Group::Hand(Side::You)),
        CardHandling::Pile {
            side,
            pile: ZoneName::Hand,
            ..
        } => Some(Group::Hand(side)),
        // The opponent's hand, which the viewer cannot see into, and a pile the table does not
        // place (a sideboard), show nothing lifted.
        CardHandling::Hand { .. } | CardHandling::Pile { .. } => None,
    }
}

/// The cards `handling` takes from `members`, the objects of its group as the group lists them
/// (a library bottom first, a hand in order), in the order they leave it: a library's from the
/// top down.
pub(crate) fn handled<'a>(
    handling: CardHandling,
    members: &[&'a ObjectView],
) -> Vec<&'a ObjectView> {
    match handling {
        CardHandling::Library { count, .. } => members
            .iter()
            .rev()
            .take(usize::from(count.unwrap_or(0)))
            .copied()
            .collect(),
        CardHandling::Hand { .. } => members.to_vec(),
        CardHandling::Pile {
            pile,
            count,
            bottom,
            ..
        } => {
            let mut cards: Vec<&ObjectView> = members.to_vec();
            if pile == ZoneName::Library {
                cards.reverse();
            }
            let count = usize::try_from(count)
                .unwrap_or(usize::MAX)
                .min(cards.len());
            if bottom {
                cards.split_off(cards.len() - count)
            } else {
                cards.truncate(count);
                cards
            }
        }
    }
}

/// A look at cards and the cards it holds up. Which cards those are is decided as the look
/// begins, as the web's `handledAt` does: a card that later leaves the pile is no longer held
/// up, and the card that takes its place on the pile is not lifted for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Handled {
    pub handling: CardHandling,
    /// The cards held up, in the order they left their pile.
    pub slots: Vec<ObjectId>,
}

impl Handled {
    /// The look `handling` as it begins over `zones`.
    pub(crate) fn of(zones: &Zones, handling: CardHandling) -> Self {
        let slots = group_of(handling).map_or_else(Vec::new, |group| {
            handled(handling, &members(zones, group))
                .iter()
                .map(|object| object.slot)
                .collect()
        });
        Self { handling, slots }
    }
}

/// Moves the cards of `group` that `handled` holds up, among `members`, to where they rest
/// while they are handled, keeping the rest of each card's resting place as its group gave it.
/// Cards of a library lie face down beside it on the table, at its size and raised off it, as
/// Arena's do, fanned toward the middle of the table. Cards of a hand are held up in front of
/// the camera, in the middle of the screen.
pub(crate) fn lay(
    handled: Option<&Handled>,
    group: Group,
    members: &[&ObjectView],
    (layout, camera): (&Layout, &Camera),
    placement: &mut Placement,
) {
    let Some(handled) = handled.filter(|handled| group_of(handled.handling) == Some(group)) else {
        return;
    };
    let cards: Vec<&ObjectView> = handled
        .slots
        .iter()
        .filter_map(|slot| members.iter().find(|object| object.slot == *slot).copied())
        .collect();
    if cards.is_empty() {
        return;
    }
    let side = match group {
        Group::Pile { side, .. } | Group::Hand(side) | Group::Row { side, .. } => side,
        Group::Stack => Side::You,
    };
    let library = matches!(group, Group::Pile { .. });
    let columns = if library {
        lift::library::COLUMNS
    } else {
        lift::hand::COLUMNS
    }
    .min(cards.len());
    let packet = cards.len().div_ceil(columns);
    #[expect(clippy::cast_precision_loss, reason = "a handful of cards")]
    for (index, object) in cards.iter().enumerate() {
        let (column, under) = (index % columns, index / columns);
        let offset = column as f32 - (columns as f32 - 1.0) / 2.0;
        // Each column lies over the one at its left, and within a column the cards of one
        // packet lie under the first.
        let order = (column * packet + packet - under) as f32;
        let pose = if library {
            let beside = lift::library::BESIDE
                + Vec2::X * offset * lift::library::STEP * CARD_WIDTH
                + lift::library::PACKET * under as f32;
            // Beside the library toward the table's middle: to its right on the viewer's side,
            // and to its left on the opponent's.
            // The lifted cards are as large as the pile's, and lie as far apart for their size.
            let scale = super::pile::pile_scale(layout, side);
            let beside = match side {
                Side::You => beside,
                Side::Opponent => -beside,
            } * scale;
            let at = super::pile::pile_base(layout, camera, side, ZoneName::Library) + beside;
            Pose {
                position: at.extend(lift::library::RISE + LAYER * order),
                scale,
                turn: 1.0,
                ..Pose::default()
            }
        } else {
            held_up(object, side, (offset, under, order), camera)
        };
        if let Some(rest) = placement.get(object.slot).copied() {
            // A card lifted from under the top two of a pile is drawn, as the pile's are not.
            placement.set(
                object.slot,
                super::Rest {
                    pose,
                    visible: true,
                    ..rest
                },
            );
        }
    }
}

/// Where a card taken from `side`'s hand is held up in front of the camera, as the web's
/// `handlingPlacements` holds it: the viewer's low in the middle of the screen, the opponent's
/// high. `offset` is its column's distance from the middle column, `under` how many cards of its
/// column lie over it, and `order` how far it lies over the first card. The opponent's cards show
/// their backs.
fn held_up(
    object: &ObjectView,
    side: Side,
    (offset, under, order): (f32, usize, f32),
    camera: &Camera,
) -> Pose {
    let aspect = camera.window.size.width / camera.window.size.height;
    // A card's width, in normalized device coordinates of the window.
    let width = lift::hand::HEIGHT * 2.0 * (CARD_WIDTH / CARD_HEIGHT) / aspect;
    let at = match side {
        Side::You => lift::hand::AT,
        Side::Opponent => lift::hand::THEIRS_AT,
    };
    #[expect(clippy::cast_precision_loss, reason = "a handful of cards")]
    let ndc =
        at + Vec2::new(
            offset * lift::hand::STEP * width,
            -offset.abs() * lift::hand::DROP,
        ) + lift::hand::PACKET * under as f32;
    let rotation = Quat::from_rotation_z(-(offset * lift::hand::LEAN_DEGREES).to_radians());
    let depth = lift::hand::DEPTH - order;
    let mut pose = camera.screen_pose(ndc, depth, rotation, lift::hand::HEIGHT);
    pose.turn = if object.owner == Side::Opponent || !super::rows::face_up(object) {
        1.0
    } else {
        0.0
    };
    pose
}

/// Lays out `picked`, the cards of `side`'s hand its player picked in a hidden pick of the
/// resolution that is paused, apart from the hand where the cards a player handles from their
/// hand are held up: face up for their player, face down for the other ([CR 101.4a]): "each
/// player must clearly indicate which face-down card they are choosing." The web interface's
/// `pickedPlacements` (`ui/src/game/layout.ts`) lays them out the same way.
///
/// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
pub(crate) fn lay_picked(
    picked: &[&ObjectView],
    side: Side,
    camera: &Camera,
    placement: &mut Placement,
) {
    let columns = lift::hand::COLUMNS.min(picked.len());
    if columns == 0 {
        return;
    }
    let packet = picked.len().div_ceil(columns);
    #[expect(clippy::cast_precision_loss, reason = "a handful of cards")]
    for (index, object) in picked.iter().enumerate() {
        let (column, under) = (index % columns, index / columns);
        let offset = column as f32 - (columns as f32 - 1.0) / 2.0;
        let order = (column * packet + packet - under) as f32;
        let pose = held_up(object, side, (offset, under, order), camera);
        placement.set(
            object.slot,
            super::Rest {
                pose,
                place: Place::Card {
                    zone: ZoneName::Hand,
                    side,
                    row: None,
                },
                visible: true,
                copies: 1,
            },
        );
    }
}

/// The look open as a script plays, for the table it plays over, which does not keep it.
#[derive(Debug)]
pub(crate) struct Look {
    /// The cards held up as the steps so far leave the table.
    handled: Option<Handled>,
    /// Whether the script changes to the look `next` with no step of its own saying so, and
    /// has not yet.
    changes: bool,
    next: Option<Handled>,
    /// Whether no step of the script takes time, so that the first step makes that change.
    instant: bool,
    /// The cards the step being played lifts as a look begins, or puts back as it ends.
    lifting: Vec<ObjectId>,
}

impl Look {
    /// The look open as a script from `before` to `after` starts, `timed` if any of its steps
    /// takes time. A script with a `lift` step follows the look from the one `before` shows.
    /// The session also begins, changes and ends looks with no step of the script saying so: a
    /// look an ability's resolution asks for, which only the views show the other seat, counting
    /// fewer cards as the player takes some. Such a script changes to the look `after` shows at
    /// its first step that takes time, so that the cards that change place move over that step,
    /// and each card ends where `after` has it.
    pub(crate) fn opening((before, after): (&PlayerView, &PlayerView), timed: bool) -> Self {
        let announced = after
            .script
            .iter()
            .flat_map(|script| &script.beats)
            .flatten()
            .any(|step| matches!(step.act, Act::Lift { .. }));
        // Which cards a look holds up does not depend on the stack, so its abilities need no
        // numbers here. A view with no look needs no zones.
        let of = |view: &PlayerView| {
            view.handling
                .and_then(|_| Zones::of_view(view, &[]).handling)
        };
        let handled = of(before);
        let open = if announced { None } else { of(after) };
        Self {
            changes: !announced && open != handled,
            next: open,
            handled,
            instant: !timed,
            lifting: Vec::new(),
        }
    }

    /// The cards held up as the steps so far leave the table.
    pub(crate) fn handled(&self) -> Option<Handled> {
        self.handled.clone()
    }

    /// The cards the step being played lifts as a look begins, or puts back as it ends.
    pub(crate) fn lifting(&self) -> &[ObjectId] {
        &self.lifting
    }

    /// Follows the look as `act`, applied to `played`, changes it, and marks in `dirty`, by
    /// `Group::index`, the groups whose cards it lifts or puts back. `timed` says whether the
    /// step, or the room it makes, takes time.
    pub(crate) fn follow(
        &mut self,
        (act, timed): (&Act, bool),
        played: &Played,
        dirty: &mut [bool],
    ) {
        self.lifting.clear();
        let mut mark = |handled: &Handled| {
            if let Some(group) = group_of(handled.handling) {
                dirty[group.index()] = true;
            }
        };
        if self.changes && (timed || self.instant) {
            self.changes = false;
            self.handled
                .iter()
                .chain(self.next.iter())
                .for_each(&mut mark);
            self.handled = self.next.take();
        }
        // A look lifts cards off their pile, or puts them back, naming none of them.
        if let Act::Lift { span, handling } = act {
            let ended = self.handled.take();
            if *span == Span::Begin {
                self.handled = Some(Handled::of(&Zones::of_table(played), *handling));
            }
            let moved = match span {
                Span::Begin => self.handled.as_ref(),
                Span::End => ended.as_ref(),
            };
            self.lifting
                .extend(moved.iter().flat_map(|handled| &handled.slots));
            ended.iter().chain(self.handled.iter()).for_each(&mut mark);
        }
    }
}
