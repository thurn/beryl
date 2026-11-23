//! The opening's gold disc, from the web's `Coin` and `mana-pool-and-coin.css`.

use std::f32::consts::{FRAC_PI_6, TAU};

use game_table::profiles::Profile;
use glam::Vec2;
use kit_base::pose::Curve;
use kit_base::tokens::{color, gradient, ratio, shadow, size, type_style};
use kit_base::typography::TypeStyle;
use kit_ui::element::{Align, Element, Justify, row, text};

use super::scaled_shadows;

/// Name of the disc's element, which the client poses on the frame clock.
pub const NAME: &str = "opening coin";

/// The disc's diameter in logical points, as `OpeningPresentation` sizes it.
#[must_use]
pub fn diameter(profile: Profile) -> f32 {
    let width = profile.window().size.width;
    profile.data().hud.opening.wrapped_hand.map_or_else(
        || size::OPENING_COIN_DIAMETER * profile.screen_projection().scale,
        |hand| {
            if width < hand.narrow_below {
                hand.coin_narrow
            } else {
                hand.coin
            }
        },
    )
}

/// The coin occupies a fixed slot, including while it turns edge on.
pub(super) fn disc(diameter: f32, first: bool) -> Element {
    let scale = diameter / size::OPENING_COIN_DIAMETER;
    let numeral = type_style::OPENING_COIN_NUMBER;
    let mark = first.then(|| {
        text("1")
            .style(TypeStyle {
                size: numeral.size * scale,
                ..numeral
            })
            .color(color::OPENING_COIN_NUMBER)
    });
    row()
        .name(NAME)
        .width(diameter)
        .height(diameter)
        .shrink(0.0)
        .radius(diameter / 2.0)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .background_image(vec![gradient::OPENING_COIN])
        .box_shadow(scaled_shadows(&shadow::OPENING_COIN, scale))
        .passes(true)
        .children(mark)
}

/// The three motions of the opening coin, as it waits for and then shows who starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Spin,
    Rock,
    Land,
}

impl Motion {
    /// The coin's vertical offset, apparent scale and whether another frame is needed.
    /// A turn about X or Y is projected as a foreshortened disc.
    #[must_use]
    pub fn pose(self, elapsed: u64, diameter: f32) -> (Vec2, Vec2, bool) {
        const LAND: u64 = 900_000_000;
        const SPIN: u64 = 800_000_000;
        const ROCK: u64 = 1_400_000_000;
        const LAND_EASE: Curve = Curve {
            x1: ratio::OPENING_COIN_LAND_EASE_X1,
            y1: ratio::OPENING_COIN_LAND_EASE_Y1,
            x2: ratio::OPENING_COIN_LAND_EASE_X2,
            y2: 1.0,
        };
        let em = diameter * (type_style::OPENING_COIN_NUMBER.size / size::OPENING_COIN_DIAMETER);
        let edge = |angle: f32| angle.cos().abs().max(ratio::OPENING_COIN_EDGE_SCALE);
        match self {
            Self::Spin => (
                Vec2::ZERO,
                Vec2::new(edge(TAU * fraction(elapsed % SPIN, SPIN)), 1.0),
                true,
            ),
            Self::Rock => {
                let cycle = fraction(elapsed % (2 * ROCK), ROCK);
                let progress = Curve::EASE_IN_OUT.ease(1.0 - (1.0 - cycle).abs());
                let tilt = 1.0 - 2.0 * progress;
                (
                    Vec2::Y * (-ratio::OPENING_COIN_ROCK_RISE * em * tilt),
                    Vec2::new(1.0, (FRAC_PI_6 * tilt).cos()),
                    true,
                )
            }
            Self::Land if elapsed < LAND => {
                let progress = LAND_EASE.ease(fraction(elapsed, LAND));
                (
                    Vec2::Y * (-ratio::OPENING_COIN_LAND_RISE * em * (1.0 - progress)),
                    Vec2::new(edge(ratio::OPENING_COIN_LAND_TURNS * TAU * progress), 1.0),
                    true,
                )
            }
            Self::Land => (Vec2::ZERO, Vec2::ONE, false),
        }
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a fraction of one animation cycle"
)]
fn fraction(elapsed: u64, duration: u64) -> f32 {
    elapsed as f32 / duration as f32
}
