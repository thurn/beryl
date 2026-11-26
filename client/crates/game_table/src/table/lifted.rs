//! Where the cards the opponent is handling rest while they look at them (`lift`): the top cards
//! of a library, lying face down beside it as Arena's do, or a hand, held up in front of the
//! camera in a small fan as the web interface's `handlingPlacements` (`ui/src/game/layout.ts`)
//! holds them: smaller and high at the left before the first turn, when the opening hand is
//! looked at. A card of a hand shows its back when the opponent owns it or a counted look at a
//! pile takes it, as the web's do, and otherwise the face its record shows to the viewer.
//!
//! The handling is the view's (`PlayerView::handling`), or, while a script plays, the one its
//! `lift` steps so far have begun. Which cards it holds follows the web's `handledAt`
//! (`ui/src/stage/table.ts`).

use std::ops::Range;

use beryl_protocol::protocol::{CardHandling, ObjectId, ObjectView, PlayerView, Side, ZoneName};
use beryl_protocol::script::{Act, Span, Table as Played};
use glam::Vec2;

use super::place::{Group, Placement, Zones, members};
use super::{Camera, Layout, Place};
use kit_base::data::verbs::{Lift, LiftHand};
use kit_base::pose::Pose;
use kit_space::arrange::{LiftedFan, LiftedPlane};

/// The group that holds the cards `handling` takes, if the table places any of them. On a
/// table that shares its piles, a library's side names the player who looks, and the cards
/// come from the one shared library.
#[must_use]
pub(crate) fn group_of(handling: CardHandling, shared: bool) -> Option<Group> {
    match handling {
        CardHandling::Library { side, .. }
        | CardHandling::Pile {
            side,
            pile: ZoneName::Library,
            ..
        } => Some(Group::Pile {
            side: (!shared).then_some(side),
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
pub struct Handled {
    pub(crate) handling: CardHandling,
    /// The cards held up, in the order they left their pile.
    pub(crate) slots: Vec<ObjectId>,
    /// Whether the look is before the game's first turn, at an opening hand, which the web's
    /// `handlingPlacements` holds up smaller and out of the way (`view.turn.number === 0`).
    pub(crate) opening: bool,
    /// Whether the table shares its library, so that a look at a library takes the shared one.
    pub(crate) shared: bool,
}

impl Handled {
    /// The look a snapshot places. The session only lifts a counted look at the top of a
    /// library for a scry (UI Conventions, "Cards a look returns"). During other choices
    /// the library stays in place until a step moves a card from it.
    pub(crate) fn of_view(
        zones: &Zones,
        handling: Option<CardHandling>,
        opening: bool,
    ) -> Option<Self> {
        handling
            .filter(|handling| {
                !matches!(
                    handling,
                    CardHandling::Library {
                        count: Some(_),
                        scry: None | Some(false),
                        ..
                    }
                )
            })
            .map(|handling| Self::of(zones, handling, opening))
    }

    /// The look `handling` as it begins over `zones`, `opening` if before the first turn.
    pub(crate) fn of(zones: &Zones, handling: CardHandling, opening: bool) -> Self {
        let shared = zones.shared;
        let slots = group_of(handling, shared).map_or_else(Vec::new, |group| {
            handled(handling, &members(zones, group))
                .iter()
                .map(|object| object.slot)
                .collect()
        });
        Self {
            handling,
            slots,
            opening,
            shared,
        }
    }

    /// The group the cards held up come from, if the table places any of them.
    pub(crate) fn group(&self) -> Option<Group> {
        group_of(self.handling, self.shared)
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
    (layout, camera, lift): (&Layout, &Camera, &Lift),
    placement: &mut Placement,
) {
    let Some(handled) = handled.filter(|handled| handled.group() == Some(group)) else {
        return;
    };
    let cards: Vec<&ObjectView> = handled
        .slots
        .iter()
        .filter_map(|slot| members.iter().find(|object| object.slot == *slot).copied())
        .collect();
    let (fan, plane, band, backs) = match group {
        Group::Pile { side, .. } => {
            let library = lift.pile;
            let scale = super::pile::nominal_scale(layout, side);
            let (direction, band, toward_looker) = match side {
                Some(Side::You) => (1.0, -1.0..0.0, 0.0),
                Some(Side::Opponent) => (-1.0, 0.0..1.0, 0.0),
                // The shared library stands at one edge of the table, between the two players.
                // Its cards lie beside it toward the middle of the table, and on the side of it
                // of the player who looks: above it for the opponent, below it for the viewer.
                None => {
                    let toward_middle = if layout.shared_piles[0].x <= 0.0 {
                        1.0
                    } else {
                        -1.0
                    };
                    let (CardHandling::Library { side: looker, .. }
                    | CardHandling::Hand { side: looker }
                    | CardHandling::Pile { side: looker, .. }) = handled.handling;
                    match looker {
                        Side::You => (toward_middle, -1.0..0.0, -1.0),
                        Side::Opponent => (toward_middle, 0.0..1.0, 1.0),
                    }
                }
            };
            // A short look follows Arena: cards lie at library size beside the pile,
            // toward the middle of the table (UI Conventions, "Cards the opponent looks at").
            let pile = placement.piles[super::PileShown::index(side, ZoneName::Library)];
            let beside = pile
                .top
                .and_then(|slot| placement.get(slot))
                .map(|rest| rest.pose);
            // Beside a shared library, the cards also lie half a card toward the player who
            // looks, so that the look reads as that player's.
            let card_height = layout.camera.card.height * scale;
            let at = pile.base
                + library.beside * scale * direction
                + Vec2::Y * (card_height / 2.0) * toward_looker;
            (
                LiftedFan {
                    columns: library.columns,
                    step: library.step,
                    drop: 0.0,
                    lean: 0.0,
                },
                LiftedPlane::Table {
                    anchor: Pose {
                        position: at.extend(library.rise),
                        scale,
                        ..Pose::default()
                    },
                    direction,
                    depth_step: layout.camera.card.layer,
                    beside,
                },
                band,
                true,
            )
        }
        Group::Hand(side) => {
            let (fan, plane, band) =
                held_up(side, handled.opening, layout.hand.ceiling, &lift.hand);
            (
                fan,
                plane,
                band,
                matches!(handled.handling, CardHandling::Pile { .. }),
            )
        }
        _ => return,
    };
    for (object, mut pose) in cards
        .iter()
        .zip(fan.poses(camera, cards.len(), plane, band))
    {
        pose.turn = held_turn(object, backs);
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
    // The top two of the cards left on a pile are drawn, as `lay_pile` draws a pile's top two,
    // or the pile shows only the dark rims of the cards beneath them, a black card where its
    // top card should be.
    if matches!(group, Group::Pile { .. }) {
        let remaining = members
            .iter()
            .rev()
            .filter(|object| !handled.slots.contains(&object.slot));
        for object in remaining.take(2) {
            if let Some(rest) = placement.get(object.slot).copied() {
                placement.set(
                    object.slot,
                    super::Rest {
                        visible: true,
                        ..rest
                    },
                );
            }
        }
    }
}

/// The anchor for a hand held in front of the camera. The viewer's regular hand stays
/// below the profile's ceiling so the back row retains a strip to press.
fn held_up(
    side: Side,
    upper_corner: bool,
    ceiling: f32,
    hand: &LiftHand,
) -> (LiftedFan, LiftedPlane, Range<f32>) {
    let (at, height, band) = match (upper_corner, side) {
        (true, _) => (hand.opening.at, hand.opening.height, 0.0..1.0),
        (false, Side::You) if ceiling.is_finite() => (
            Vec2::new(hand.at.x, ceiling - hand.height),
            hand.height,
            -1.0..ceiling,
        ),
        (false, Side::You) => (hand.at, hand.height, -1.0..0.0),
        (false, Side::Opponent) => (hand.theirs_at, hand.height, 0.0..1.0),
    };
    (
        LiftedFan {
            columns: hand.columns,
            step: hand.step,
            drop: hand.drop,
            lean: hand.lean_degrees.to_radians(),
        },
        LiftedPlane::Facing {
            at,
            depth: hand.depth,
            height,
        },
        band,
    )
}

fn held_turn(object: &ObjectView, backs: bool) -> f32 {
    if backs || object.owner == Side::Opponent || !super::rows::face_up(object) {
        1.0
    } else {
        0.0
    }
}

/// Lays out the cards a player picked while a resolution is paused, apart from their hand:
/// face up for their player and face down for the other ([CR 101.4a]): "each player must
/// clearly indicate which face-down card they are choosing." The viewer's picks use the
/// held-up hand position, as in `pickedPlacements` (`ui/src/game/layout.ts`). On the desktop
/// the opponent's picks use the upper corner where a held-up opening hand lies, clear of
/// the permanents the viewer may still need to choose.
///
/// [CR 101.4a]: https://yawgatog.com/resources/magic-rules/#R1014a
pub(crate) fn lay_picked(
    picked: &[&ObjectView],
    side: Side,
    (layout, camera, hand): (&Layout, &Camera, &LiftHand),
    placement: &mut Placement,
) {
    let upper_corner = side == Side::Opponent && layout.bands.is_none();
    let (fan, plane, band) = held_up(side, upper_corner, layout.hand.ceiling, hand);
    for (object, mut pose) in picked
        .iter()
        .zip(fan.poses(camera, picked.len(), plane, band))
    {
        pose.turn = held_turn(object, false);
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
                overlapped: false,
            },
        );
    }
}

/// The look open as a script plays, for the table it plays over, which does not keep it.
#[derive(Debug)]
pub struct Look {
    /// The cards held up as the steps so far leave the table.
    handled: Option<Handled>,
    /// Whether the script changes to the look `next` with no step of its own saying so, and
    /// has not yet.
    changes: bool,
    next: Option<Handled>,
    /// Whether the script plays before the first turn, so that a look it begins is at an
    /// opening hand.
    opening: bool,
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
    pub fn opening((before, after): (&PlayerView, &PlayerView), timed: bool) -> Self {
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
        let opening = after.turn.number == 0;
        let handled = of(before);
        let open = if announced { None } else { of(after) };
        Self {
            changes: !announced && open != handled,
            next: open,
            opening,
            handled,
            instant: !timed,
            lifting: Vec::new(),
        }
    }

    /// The cards held up as the steps so far leave the table.
    pub fn handled(&self) -> Option<Handled> {
        self.handled.clone()
    }

    /// The cards the step being played lifts as a look begins, or puts back as it ends.
    pub fn lifting(&self) -> &[ObjectId] {
        &self.lifting
    }

    /// Follows the look as `act`, applied to `played`, changes it, and marks in `dirty`, by
    /// `Group::index`, the groups whose cards it lifts or puts back. `timed` says whether the
    /// step, or the room it makes, takes time. `arrival` is the order the hands of `played`
    /// list their cards in (`table::arrival`): a look at a hand holds its cards up in that
    /// order, the order the view the script leads to rests them in, where the table played over
    /// keeps them by slot.
    pub fn follow(
        &mut self,
        (act, timed): (&Act, bool),
        (played, arrival): (&Played, &[u32]),
        dirty: &mut [bool],
    ) {
        self.lifting.clear();
        let mut mark = |handled: &Handled| {
            if let Some(group) = handled.group() {
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
                self.handled = Some(Handled::of(
                    &Zones::of_table(played).arrived(arrival),
                    *handling,
                    self.opening,
                ));
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
