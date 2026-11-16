//! SVG path data (the `d` attribute of a `<path>`) read into a `tiny_skia` path, so that the
//! icons can be kept as the web interface's own path strings, copied exactly. Every command of
//! SVG 1.1's path grammar is read, absolute and relative: moves, lines, horizontal and vertical
//! lines, cubic and quadratic curves with their smooth forms, elliptical arcs and closes. An arc
//! becomes cubic curves of at most a quarter turn each, by the conversion in SVG 1.1's
//! implementation notes (F.6, "Elliptical arc implementation notes").

use std::f32::consts::{FRAC_PI_2, TAU};

use tiny_skia::{Path, PathBuilder};

/// The path that `data` describes, or `None` when it is not valid path data or draws nothing.
#[must_use]
pub fn parse(data: &str) -> Option<Path> {
    let mut reader = Reader {
        bytes: data.as_bytes(),
        at: 0,
    };
    let mut pen = Pen::default();
    let mut command = None;
    loop {
        reader.skip_separators();
        let Some(&next) = reader.bytes.get(reader.at) else {
            break;
        };
        if next.is_ascii_alphabetic() {
            reader.at += 1;
            command = Some(next);
        } else if command.is_none() {
            return None;
        }
        let letter = command?;
        pen.command(letter, &mut reader)?;
        // A move's further coordinate pairs are lines, by the grammar's rule.
        command = match letter {
            b'M' => Some(b'L'),
            b'm' => Some(b'l'),
            b'Z' | b'z' => None,
            other => Some(other),
        };
    }
    pen.builder.finish()
}

/// Where the pen is, and the path drawn so far.
#[derive(Default)]
struct Pen {
    builder: PathBuilder,
    at: (f32, f32),
    /// Where the current subpath started, which a close returns to.
    start: (f32, f32),
    /// The last curve's second control point, which a smooth curve reflects: of a cubic for
    /// `S`, of a quadratic for `T`, and `None` after any other command.
    cubic: Option<(f32, f32)>,
    quadratic: Option<(f32, f32)>,
}

impl Pen {
    /// Reads one command's arguments from `reader` and draws it.
    fn command(&mut self, letter: u8, reader: &mut Reader) -> Option<()> {
        let relative = letter.is_ascii_lowercase();
        let origin = if relative { self.at } else { (0.0, 0.0) };
        let point = |reader: &mut Reader| -> Option<(f32, f32)> {
            Some((origin.0 + reader.number()?, origin.1 + reader.number()?))
        };
        let (mut cubic, mut quadratic) = (None, None);
        match letter.to_ascii_uppercase() {
            b'M' => {
                let to = point(reader)?;
                self.builder.move_to(to.0, to.1);
                self.start = to;
                self.at = to;
            }
            b'L' => self.line(point(reader)?),
            b'H' => {
                let x = origin.0 + reader.number()?;
                self.line((x, self.at.1));
            }
            b'V' => {
                let y = if relative { self.at.1 } else { 0.0 } + reader.number()?;
                self.line((self.at.0, y));
            }
            b'C' => {
                let (first, second, to) = (point(reader)?, point(reader)?, point(reader)?);
                self.cubic(first, second, to);
                cubic = Some(second);
            }
            b'S' => {
                let first = reflect(self.cubic, self.at);
                let (second, to) = (point(reader)?, point(reader)?);
                self.cubic(first, second, to);
                cubic = Some(second);
            }
            b'Q' => {
                let (control, to) = (point(reader)?, point(reader)?);
                self.quadratic(control, to);
                quadratic = Some(control);
            }
            b'T' => {
                let control = reflect(self.quadratic, self.at);
                let to = point(reader)?;
                self.quadratic(control, to);
                quadratic = Some(control);
            }
            b'A' => {
                let radii = (reader.number()?, reader.number()?);
                let rotation = reader.number()?;
                let large = reader.flag()?;
                let sweep = reader.flag()?;
                let to = point(reader)?;
                self.arc(radii, rotation, large, sweep, to);
            }
            b'Z' => {
                self.builder.close();
                self.at = self.start;
            }
            _ => return None,
        }
        self.cubic = cubic;
        self.quadratic = quadratic;
        Some(())
    }

    fn line(&mut self, to: (f32, f32)) {
        self.builder.line_to(to.0, to.1);
        self.at = to;
    }

    fn cubic(&mut self, first: (f32, f32), second: (f32, f32), to: (f32, f32)) {
        (self.builder).cubic_to(first.0, first.1, second.0, second.1, to.0, to.1);
        self.at = to;
    }

    fn quadratic(&mut self, control: (f32, f32), to: (f32, f32)) {
        self.builder.quad_to(control.0, control.1, to.0, to.1);
        self.at = to;
    }

    /// An elliptical arc from the pen to `to` with `radii`, its x axis turned `rotation`
    /// degrees, the larger or smaller of the two arcs that fit by `large`, drawn clockwise in
    /// SVG's y-down space when `sweep`. Radii too small to reach are scaled up until they do.
    fn arc(&mut self, radii: (f32, f32), rotation: f32, large: bool, sweep: bool, to: (f32, f32)) {
        let from = self.at;
        let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
        if from == to {
            return;
        }
        if rx == 0.0 || ry == 0.0 {
            self.line(to);
            return;
        }
        let (sin, cos) = rotation.to_radians().sin_cos();
        // The midpoint between the ends, in the ellipse's own axes (F.6.5, step 1).
        let half = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
        let x1 = cos * half.0 + sin * half.1;
        let y1 = -sin * half.0 + cos * half.1;
        let reach = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
        if reach > 1.0 {
            (rx, ry) = (rx * reach.sqrt(), ry * reach.sqrt());
        }
        // The centre (steps 2 and 3).
        let (rx2, ry2) = (rx * rx, ry * ry);
        let numerator = rx2 * ry2 - rx2 * y1 * y1 - ry2 * x1 * x1;
        let denominator = rx2 * y1 * y1 + ry2 * x1 * x1;
        let side = if large == sweep { -1.0 } else { 1.0 };
        let root = side * (numerator / denominator).max(0.0).sqrt();
        let (cx1, cy1) = (root * rx * y1 / ry, -root * ry * x1 / rx);
        let centre = (
            cos * cx1 - sin * cy1 + f32::midpoint(from.0, to.0),
            sin * cx1 + cos * cy1 + f32::midpoint(from.1, to.1),
        );
        // The start angle and the angle swept (step 4).
        let u = ((x1 - cx1) / rx, (y1 - cy1) / ry);
        let v = ((-x1 - cx1) / rx, (-y1 - cy1) / ry);
        let start = angle((1.0, 0.0), u);
        let mut swept = angle(u, v);
        if !sweep && swept > 0.0 {
            swept -= TAU;
        } else if sweep && swept < 0.0 {
            swept += TAU;
        }
        // Each piece of at most a quarter turn is one cubic, whose control points lie along
        // the tangents at a length of 4/3 tan(angle / 4).
        let pieces = (swept.abs() / FRAC_PI_2).ceil().max(1.0);
        let step = swept / pieces;
        let length = 4.0 / 3.0 * (step / 4.0).tan();
        let place = |(x, y): (f32, f32)| {
            (
                centre.0 + cos * rx * x - sin * ry * y,
                centre.1 + sin * rx * x + cos * ry * y,
            )
        };
        let mut a = start;
        for _ in 0..pieces as u32 {
            let b = a + step;
            let (sin_a, cos_a) = a.sin_cos();
            let (sin_b, cos_b) = b.sin_cos();
            let first = place((cos_a - length * sin_a, sin_a + length * cos_a));
            let second = place((cos_b + length * sin_b, sin_b - length * cos_b));
            let end = place((cos_b, sin_b));
            self.cubic(first, second, end);
            a = b;
        }
        // The last piece ends where the arc must, whatever rounding did.
        self.at = to;
    }
}

/// The reflection of `control` about `at`, or `at` itself with no control point to reflect.
fn reflect(control: Option<(f32, f32)>, at: (f32, f32)) -> (f32, f32) {
    control.map_or(at, |(x, y)| (2.0 * at.0 - x, 2.0 * at.1 - y))
}

/// The signed angle from `u` to `v`, in radians.
fn angle(u: (f32, f32), v: (f32, f32)) -> f32 {
    (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1)
}

/// A cursor over path data.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    /// Skips whitespace and commas.
    fn skip_separators(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            self.at += 1;
        }
    }

    /// Reads a number: an optional sign, digits with at most one decimal point, and an
    /// optional exponent. A second point or a sign ends it, so `1.5.5` is two numbers and
    /// `4-6` is `4` and `-6`, as the grammar allows.
    fn number(&mut self) -> Option<f32> {
        self.skip_separators();
        let start = self.at;
        let mut end = start;
        let byte = |i: usize| self.bytes.get(i).copied();
        if matches!(byte(end), Some(b'-' | b'+')) {
            end += 1;
        }
        let mut point = false;
        while let Some(next) = byte(end) {
            if next.is_ascii_digit() {
                end += 1;
            } else if next == b'.' && !point {
                point = true;
                end += 1;
            } else {
                break;
            }
        }
        if matches!(byte(end), Some(b'e' | b'E')) {
            let mut exponent = end + 1;
            if matches!(byte(exponent), Some(b'-' | b'+')) {
                exponent += 1;
            }
            if byte(exponent).is_some_and(|next| next.is_ascii_digit()) {
                end = exponent;
                while byte(end).is_some_and(|next| next.is_ascii_digit()) {
                    end += 1;
                }
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..end]).ok()?;
        let value = text.parse().ok()?;
        self.at = end;
        Some(value)
    }

    /// Reads an arc's flag, a single `0` or `1`, which needs no separator after it.
    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let value = match self.bytes.get(self.at)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.at += 1;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn bounds(data: &str) -> [f32; 4] {
        let rect = parse(data).expect("valid path data").bounds();
        [rect.left(), rect.top(), rect.right(), rect.bottom()]
    }

    fn near(found: [f32; 4], expected: [f32; 4]) -> bool {
        found
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 0.01)
    }

    #[test]
    fn relative_lines_and_implicit_repeats_reach_where_the_grammar_says() {
        // The close button's X: a move followed by bare pairs, then relative lines.
        let x = "M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 \
                 6.4-6.4-6.4-6.4z";
        assert!(near(bounds(x), [3.5, 3.5, 20.5, 20.5]), "{:?}", bounds(x));
    }

    #[test]
    fn a_half_circle_arc_bulges_by_its_radius() {
        // The end phase's setting sun: a half disc of radius 6.5 above y = 17.5.
        let sun = "M5.5 17.5 A6.5 6.5 0 0 1 18.5 17.5 Z";
        assert!(
            near(bounds(sun), [5.5, 11.0, 18.5, 17.5]),
            "{:?}",
            bounds(sun)
        );
    }

    #[test]
    fn compact_numbers_and_arc_flags_are_split() {
        // `.5.5` is two numbers, and the flags `01` need no separator.
        let path = "M0 0h1.5.5V2a1 1 0 01-1 1z";
        assert!(
            near(bounds(path), [0.0, 0.0, 2.0, 3.0]),
            "{:?}",
            bounds(path)
        );
    }

    #[test]
    fn data_that_does_not_start_with_a_command_is_refused() {
        assert!(parse("10 10 L 5 5").is_none());
        assert!(parse("M0 0 L").is_none());
    }
}
