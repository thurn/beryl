//! The hands: each fanned along an edge of the screen, placed relative to the camera, as the web
//! interface's `handPlacements` and `heldPlacements` (`ui/src/game/handLayout.ts`) lay them out
//! (docs/beryl-3d.md, "Objects and placement").

use beryl_protocol::protocol::{ObjectView, Side, ZoneName};
use glam::{Quat, Vec2};

use crate::pose::Pose;

use super::place::{Placement, Rest, turn};
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, HandData};
use crate::frame::Place;

/// A hand, fanned along an edge of the screen and placed relative to the camera so that it
/// keeps its size and place at any window shape. The middle of each card's inner edge (the
/// viewer's cards' top edges, the opponent's bottom edges) lies on one circle of
/// `HandData::fan_radius` card widths, and the card turns to lie along it. Each card faces the
/// camera a little nearer the eye than the card before it, so later cards cover earlier ones.
/// The viewer's hand runs along the bottom edge, as the web interface's `handPlacements` has
/// it; the opponent's along the top, upside down to the viewer as the opponent holds it and
/// mostly out of view, as `heldPlacements` has it. A card its player picked in a hidden pick
/// lies apart from the fan (`lifted::lay_picked`). A viewer's hand too large for one fan that
/// leaves each card a strip to press is laid in several fans, one behind and above another
/// (`Fan::rows`).
pub(super) fn hand(
    placement: &mut Placement,
    objects: &[&ObjectView],
    side: Side,
    (data, strip): (HandData, f32),
    camera: &Camera,
    cache: Option<&mut HandFits>,
) {
    // A hand with no card picked, as nearly every hand is, is laid out with no copy of its list.
    let (picked, fan): (Vec<&ObjectView>, Vec<&ObjectView>);
    let objects = if objects.iter().any(|object| object.hidden_pick) {
        (picked, fan) = objects.iter().partition(|object| object.hidden_pick);
        &fan[..]
    } else {
        picked = Vec::new();
        objects
    };
    let count = objects.len();
    let place = Place::Card {
        zone: ZoneName::Hand,
        side,
        row: None,
    };
    let mut place_card = |object: &ObjectView, pose: Pose| {
        placement.set(
            object.slot,
            Rest {
                pose: Pose {
                    turn: turn(object),
                    ..pose
                },
                place,
                visible: true,
                copies: 1,
            },
        );
    };
    if let Some(cache) = cache.filter(|_| count <= HandFits::MAX_CACHED) {
        for (object, &pose) in objects
            .iter()
            .zip(cache.poses(side, count, data, strip, camera))
        {
            place_card(object, pose);
        }
    } else {
        fan_poses(count, side, data, strip, camera, |index, pose| {
            place_card(objects[index], pose);
        });
    }
    super::lifted::lay_picked(&picked, side, camera, placement);
}

/// Fans of each size for a single script, whose window and hand layout stay fixed. A script
/// can return to the same fan size after drawing and playing cards. The cache holds only small
/// hands so a script that runs through many sizes cannot use memory quadratic in its hand size.
#[derive(Default)]
pub(crate) struct HandFits([FanCache; 2]);

#[derive(Default)]
struct FanCache {
    starts: Vec<Option<usize>>,
    poses: Vec<Pose>,
}

impl HandFits {
    const MAX_CACHED: usize = 32;

    fn poses(
        &mut self,
        side: Side,
        count: usize,
        data: HandData,
        strip: f32,
        camera: &Camera,
    ) -> &[Pose] {
        let FanCache { starts, poses } = &mut self.0[usize::from(side == Side::Opponent)];
        if starts.len() <= count {
            starts.resize(count + 1, None);
        }
        let start = *starts[count].get_or_insert_with(|| {
            let start = poses.len();
            poses.reserve(count);
            fan_poses(count, side, data, strip, camera, |_, pose| poses.push(pose));
            start
        });
        &poses[start..start + count]
    }
}

/// The geometry of the cards in a fan, before taking their faces from the object records.
fn fan_poses(
    count: usize,
    side: Side,
    data: HandData,
    strip: f32,
    camera: &Camera,
    mut place: impl FnMut(usize, Pose),
) {
    if count == 0 {
        return;
    }
    let window = camera.window.size;
    let aspect = window.width / window.height;
    // Distances on screen are in half the window's height, the vertical unit of normalized
    // device coordinates, so that a turn on screen is a true turn.
    let tall = data.height * 2.0;
    let wide = tall * CARD_WIDTH / CARD_HEIGHT;
    // The viewer presses their own cards, so each keeps a strip to press; the opponent's are
    // not theirs to press, and close up within `data.room`, as the web interface's do.
    let strip = match side {
        Side::You => strip * 2.0 / window.height,
        Side::Opponent => 0.0,
    };
    let fan = Fan {
        data,
        wide,
        strip,
        aspect,
        // Four points to spare on each side of a strip, so that a square of `strip` fits in it
        // with room left where the strip's slanted sides meet the square's corners.
        margin: SPARE * 2.0 / window.height,
    };
    let (rows, spacing) = fan.rows(count);
    let radius = data.fan_radius * wide;
    let bias = data.bias_degrees.to_radians();
    // The viewer's cards hang down from their edge on the circle, and the opponent's stand up
    // from it, turned half a turn.
    let (down, half_turn) = match side {
        Side::You => (1.0, 0.0),
        Side::Opponent => (-1.0, std::f32::consts::PI),
    };
    let mut first = 0;
    for row in 0..rows {
        // The first rows take a card more than the last when the cards do not split evenly.
        let length = count / rows + usize::from(row < count % rows);
        let spacing = if length == count.div_ceil(rows) {
            spacing
        } else {
            fan.spacing(length)
        };
        // Each row stands `Fan::rise` above the one in front of it, the first row at the back.
        let behind = rows - 1 - row;
        let above = if behind == 0 {
            0.0
        } else {
            behind as f32 * fan.rise(length, spacing)
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand holds far fewer than 2^24 cards"
        )]
        let middle = (length as f32 - 1.0) / 2.0;
        for at in 0..length {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a hand holds far fewer than 2^24 cards"
            )]
            let (across, depth_index) = (at as f32, (first + at) as f32);
            let dx = (across - middle) * spacing;
            let along = dx.clamp(-radius, radius);
            let sink = radius - (radius * radius - along * along).sqrt();
            // Turned clockwise right of the middle on the viewer's side, and anticlockwise on
            // the opponent's, measured anticlockwise on screen.
            let angle = -down * ((along / radius).asin() + bias);
            let edge = Vec2::new(dx, data.edge - down * (sink - above));
            let centre = edge + Vec2::from_angle(angle).rotate(Vec2::new(0.0, -down * tall / 2.0));
            let ndc = Vec2::new(centre.x / aspect, centre.y);
            let rotation = Quat::from_rotation_z(angle + half_turn);
            // Each card stands nearer the eye than every card before it, so a later row covers
            // the foot of an earlier one.
            let depth = data.depth - depth_index * data.step_depth;
            place(
                first + at,
                camera.screen_pose(ndc, depth, rotation, data.height),
            );
        }
        first += length;
    }
}

/// The points each strip of the viewer's hand is widened by, and each row raised by, beyond what
/// a square of `strip` needs exactly: four on each side.
const SPARE: f32 = 8.0;

/// The measures a hand is fanned by, in half the window's height.
struct Fan {
    data: HandData,
    /// A card's width.
    wide: f32,
    /// The least strip of each card that the next card leaves uncovered, across the card: the
    /// layout's `strip` on the viewer's side, and nothing on the opponent's.
    strip: f32,
    /// The window's width over its height.
    aspect: f32,
    /// `SPARE`, added to each strip and each row's rise.
    margin: f32,
}

impl Fan {
    /// How many rows a hand of `count` cards is laid in: one while every card keeps its strip
    /// inside the window, and otherwise as few as let each row's cards keep theirs. A row of
    /// cards that cannot all keep a strip would leave some out of reach, so the cards fill a
    /// second row before they close up past their strips. Sixty cards take two rows in every
    /// desktop window.
    fn rows(&self, count: usize) -> (usize, f32) {
        for rows in 1..count {
            let length = count.div_ceil(rows);
            let needed = self.spacing_needed(length);
            if needed <= self.widest(length) {
                return (rows, needed);
            }
        }
        (count.max(1), 0.0)
    }

    /// How far apart the centres of a row of `count` cards stand: `data.spacing` while the row
    /// has room, and closer once the outer cards' centres would stand more than `data.room`
    /// apart. They never close up so far that a card's uncovered strip is narrower than
    /// `strip`, which `rows` makes room for, and the outer cards' inner corners, which reach
    /// farthest out once a card turns along the circle, stay inside the window.
    fn spacing(&self, count: usize) -> f32 {
        if count < 2 {
            return 0.0;
        }
        self.spacing_needed(count).min(self.widest(count))
    }

    /// The spacing of a row of `count` cards before it is held inside the window: closed up
    /// within `data.room`, but never so far that the next card covers all but `strip` of a
    /// card. A card turns along the circle, so the uncovered part of it between its side and
    /// the next card's is slanted, and a square `strip` on a side fits in it only when it is
    /// `strip` times one plus the tangent of the card's turn across. The turn grows with the
    /// spacing, so the spacing is found by repeating the sum, which settles in a few rounds
    /// because the turn grows far more slowly than the spacing.
    fn spacing_needed(&self, count: usize) -> f32 {
        if count < 2 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand holds far fewer than 2^24 cards"
        )]
        let gaps = (count - 1) as f32;
        let crowded = (self.data.spacing * self.wide).min(self.data.room * self.wide / gaps);
        if self.strip <= 0.0 {
            return crowded;
        }
        let mut spacing = crowded.max(self.strip);
        for _ in 0..4 {
            let slant = self.slant(count, spacing);
            spacing = crowded.max(self.strip * (1.0 + slant) + self.margin);
        }
        spacing
    }

    /// The widest spacing of a row of `count` cards whose outer cards' inner corners stay
    /// inside the window.
    fn widest(&self, count: usize) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand holds far fewer than 2^24 cards"
        )]
        let gaps = count.saturating_sub(1).max(1) as f32;
        2.0 * (self.aspect - self.wide / 2.0) / gaps
    }

    /// The tangent of the steepest turn of a row of `count` cards `spacing` apart: its outer
    /// cards', leaning with the fan's bias.
    fn slant(&self, count: usize, spacing: f32) -> f32 {
        let radius = self.data.fan_radius * self.wide;
        #[expect(
            clippy::cast_precision_loss,
            reason = "a hand holds far fewer than 2^24 cards"
        )]
        let along = ((count as f32 - 1.0) / 2.0 * spacing).min(radius);
        ((along / radius).asin() + self.data.bias_degrees.to_radians().abs()).tan()
    }

    /// How far a row of `count` cards `spacing` apart stands above the row in front of it, so
    /// that each of its cards keeps a square `strip` on a side above the front row's cards.
    /// Between its sides, a card's top edge falls by the slant across the strip, and the top
    /// edges of the front row's cards below it rise by as much again.
    fn rise(&self, count: usize, spacing: f32) -> f32 {
        let slant = self.slant(count, spacing);
        self.strip + 2.0 * spacing * slant + self.margin
    }
}
