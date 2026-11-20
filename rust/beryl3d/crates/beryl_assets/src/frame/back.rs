//! The card back drawn when its scan has not been downloaded: the web interface's plain card
//! back (`.card-back` in `ui/src/design/components/card-marks-and-piles.css`), a brown field in
//! a black border, which shows under Magic's card back while it loads or where images do not.

use beryl_scene::input::FaceSize;
use tiny_skia::{Pixmap, Point};

use super::FRAMES_DRAWN;
use super::layout::Box2;
use super::paint::{Canvas, color, radial, stops};
use crate::encode::dimensions;

/// The RGBA pixels at `size` of the plain card back.
#[must_use]
pub fn card_back(size: FaceSize) -> Vec<u8> {
    FRAMES_DRAWN.with(|drawn| drawn.set(drawn.get() + 1));
    let (width, height) = dimensions(size);
    let pixmap = Pixmap::new(width, height).expect("a face has an area");
    let mut canvas = Canvas::new(pixmap);
    let (width, height) = (width as f32, height as f32);
    // The corners outside the card's rounded body, as behind a drawn frame's.
    canvas.pixmap.fill(color([0x0b, 0x0a, 0x09], 1.0));
    // `border-radius: 4.8% / 3.4%`, which on a card's proportions is within a pixel of round.
    let radius = 0.048 * width;
    let card = Box2 {
        x: 0.0,
        y: 0.0,
        width,
        height,
    };
    canvas.fill_box(
        card,
        radius,
        tiny_skia::Shader::SolidColor(color([0x15, 0x11, 0x0e], 1.0)),
    );
    // `padding: 3.5%`, a share of the width on every side, and the field fills the content
    // box, its corners rounded by what the padding leaves of the card's radius.
    let padding = 0.035 * width;
    let field = card.inset(padding, padding, padding, padding);
    // `radial-gradient(ellipse at 50% 45%, #6b4426, #4a2d18 75%)`: an ellipse sized to the
    // farthest corner, which for a center at (50%, 45%) has radii of √2 times the distances to
    // the farthest sides, 50% of the width and 55% of the height.
    let center = Point::from_xy(field.x + 0.5 * field.width, field.y + 0.45 * field.height);
    let radii = (
        std::f32::consts::SQRT_2 * 0.5 * field.width,
        std::f32::consts::SQRT_2 * 0.55 * field.height,
    );
    let shade = radial(
        center,
        radii,
        stops(&[
            (0.0, [0x6b, 0x44, 0x26], 1.0),
            (0.75, [0x4a, 0x2d, 0x18], 1.0),
            (1.0, [0x4a, 0x2d, 0x18], 1.0),
        ]),
    );
    canvas.fill_box(field, (radius - padding).max(0.0), shade);
    canvas.pixmap.take()
}
