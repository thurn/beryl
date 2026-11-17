//! The arithmetic of `pixels`, with no files or frames: which pixels of two pictures of the same
//! size differ, which of them are glyphs and left out, how the rest group into regions, and how
//! many each element is answerable for on each side.

use super::Record;

/// How many levels of 255 a channel may differ by and still count as the same: `WebKit` and
/// `wgpu` blend, round and dither a little differently, and a difference this small is not visible.
pub(crate) const LEVELS: u8 = 8;

/// A region with fewer pixels than this is a speck of antialiasing along an edge that lies a
/// fraction of a pixel apart, not something one side drew wrong.
pub(crate) const SPECK: usize = 4;

/// An RGBA picture, row by row from the top.
#[derive(Clone, Copy)]
pub(crate) struct Image<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl Image<'_> {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

/// The largest difference between the color channels of `a` and `b`. Both pictures are opaque,
/// so alpha is not compared.
pub(crate) fn difference(a: [u8; 4], b: [u8; 4]) -> u8 {
    (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
}

/// A box of pixels: `left` and `top` inside it, `right` and `bottom` just past it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pixels {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Pixels {
    /// `[x, y, width, height]` in points at `scale`, cut to a picture of `width` by `height`.
    pub(crate) fn of_points(rect: [f32; 4], scale: f32, width: u32, height: u32) -> Option<Self> {
        let [x, y, w, h] = rect;
        let left = (x * scale).floor().max(0.0) as u32;
        let top = (y * scale).floor().max(0.0) as u32;
        let right = (((x + w) * scale).ceil().max(0.0) as u32).min(width);
        let bottom = (((y + h) * scale).ceil().max(0.0) as u32).min(height);
        (left < right && top < bottom).then_some(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    /// The smallest box holding both.
    pub(crate) fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }

    /// The box grown by `by` pixels on every side, cut to a picture of `width` by `height`.
    pub(crate) fn grown(self, by: u32, width: u32, height: u32) -> Self {
        Self {
            left: self.left.saturating_sub(by),
            top: self.top.saturating_sub(by),
            right: (self.right + by).min(width),
            bottom: (self.bottom + by).min(height),
        }
    }

    fn of_pixel(x: u32, y: u32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + 1,
            bottom: y + 1,
        }
    }
}

/// The box of the ink inside `line`, a line of text's box in `image`: the pixels that differ by
/// more than `LEVELS` from the color most of the box's edge has, which is what lies behind the
/// text. A line's box is as tall as its line, leading and all, so the ink is usually much
/// shorter. None when nothing in the box stands out.
pub(crate) fn ink(image: Image<'_>, line: Pixels) -> Option<Pixels> {
    let mut edge: Vec<[u8; 4]> = Vec::new();
    for x in line.left..line.right {
        edge.push(image.at(x, line.top));
        edge.push(image.at(x, line.bottom - 1));
    }
    for y in line.top..line.bottom {
        edge.push(image.at(line.left, y));
        edge.push(image.at(line.right - 1, y));
    }
    edge.sort_unstable();
    let behind = edge[edge.len() / 2];
    let mut found: Option<Pixels> = None;
    for y in line.top..line.bottom {
        for x in line.left..line.right {
            if difference(image.at(x, y), behind) > LEVELS {
                let here = Pixels::of_pixel(x, y);
                found = Some(found.map_or(here, |box_| box_.union(here)));
            }
        }
    }
    found
}

/// Which pixels are left out of the score because a glyph may be drawn there on either side:
/// every text line of both sides' records, shrunk to its ink in that side's own picture and
/// grown by a pixel for antialiasing. Glyphs are compared as data (their string, font, size
/// and box), not as pixels, because `WebKit` and Beryl 3D's text shaper place and smooth them
/// differently.
pub(crate) fn glyph_mask(sides: [(Image<'_>, &[Record]); 2], scale: f32) -> Vec<bool> {
    let (width, height) = (sides[0].0.width, sides[0].0.height);
    let mut mask = vec![false; (width * height) as usize];
    for (image, records) in sides {
        for line in records.iter().flat_map(|record| &record.lines) {
            let Some(line) = Pixels::of_points(*line, scale, width, height) else {
                continue;
            };
            let Some(inked) = ink(image, line) else {
                continue;
            };
            let inked = inked.grown(1, width, height);
            for y in inked.top..inked.bottom {
                let row = (y * width) as usize;
                mask[row + inked.left as usize..row + inked.right as usize].fill(true);
            }
        }
    }
    mask
}

/// The pixels that differ by more than `LEVELS` between `web` and `b3d` and are not masked.
pub(crate) fn differing(web: Image<'_>, b3d: Image<'_>, mask: &[bool]) -> Vec<bool> {
    (web.pixels.as_chunks::<4>().0.iter())
        .zip(b3d.pixels.as_chunks::<4>().0)
        .zip(mask)
        .map(|((a, b), &masked)| {
            !masked && difference([a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255]) > LEVELS
        })
        .collect()
}

/// The regions of `differ`: groups of differing pixels that touch along an edge. Returns each
/// pixel's region (`u32::MAX` for one that does not differ) and each region's size.
pub(crate) fn regions(differ: &[bool], width: u32) -> (Vec<u32>, Vec<usize>) {
    let width = width as usize;
    let height = differ.len() / width.max(1);
    let mut label = vec![u32::MAX; differ.len()];
    let mut sizes = Vec::new();
    let mut stack = Vec::new();
    for start in 0..differ.len() {
        if !differ[start] || label[start] != u32::MAX {
            continue;
        }
        let region = u32::try_from(sizes.len()).expect("fewer regions than pixels");
        let mut size = 0;
        label[start] = region;
        stack.push(start);
        while let Some(at) = stack.pop() {
            size += 1;
            let (x, y) = (at % width, at / width);
            let mut visit = |next: usize| {
                if differ[next] && label[next] == u32::MAX {
                    label[next] = region;
                    stack.push(next);
                }
            };
            if x > 0 {
                visit(at - 1);
            }
            if x + 1 < width {
                visit(at + 1);
            }
            if y > 0 {
                visit(at - width);
            }
            if y + 1 < height {
                visit(at + width);
            }
        }
        sizes.push(size);
    }
    (label, sizes)
}

/// For each point of a window `width` by `height` points large, the index of the topmost
/// element of `records` that paints there (`Record::paints`), by paint order (`z`), or
/// `u32::MAX`. Elements are filled into the grid from the bottom up, so a later one covers an
/// earlier one.
pub(crate) fn owners(records: &[Record], width: u32, height: u32) -> Vec<u32> {
    let mut grid = vec![u32::MAX; (width * height) as usize];
    let mut order: Vec<usize> = (0..records.len())
        .filter(|&at| records[at].paints)
        .collect();
    order.sort_by_key(|&at| records[at].z);
    for at in order {
        let Some(cells) = Pixels::of_points(records[at].rect, 1.0, width, height) else {
            continue;
        };
        let index = u32::try_from(at).expect("fewer records than u32::MAX");
        for y in cells.top..cells.bottom {
            let row = (y * width) as usize;
            grid[row + cells.left as usize..row + cells.right as usize].fill(index);
        }
    }
    grid
}

/// The differing pixels one element, or one pair of elements the matcher paired, is answerable
/// for. `web` and `b3d` are the element's index on each side, or None on a side that has no
/// such element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub web: Option<usize>,
    pub b3d: Option<usize>,
    /// Differing pixels where the web's topmost painting element is this row's.
    pub web_pixels: usize,
    /// Differing pixels where Beryl 3D's topmost painting element is this row's.
    pub b3d_pixels: usize,
    /// Of `web_pixels`, those where no Beryl 3D element paints.
    pub b3d_bare: usize,
    /// Of `b3d_pixels`, those where no web element paints.
    pub web_bare: usize,
    /// The box around all of them, once there is one.
    pub area: Option<Pixels>,
}

impl Row {
    /// How many pixels the row is answerable for: the larger of the two sides' counts.
    pub(crate) fn pixels(&self) -> usize {
        self.web_pixels.max(self.b3d_pixels)
    }

    /// Why one side may be at fault, when the other has nothing there: no such element, or no
    /// element that paints over most of the row's pixels, where only the window's background
    /// shows (the web page's own background, or Beryl 3D's table cloth, which are not
    /// elements).
    pub(crate) fn hint(&self) -> Option<&'static str> {
        match (self.web, self.b3d) {
            (Some(_), None) => Some("only the web has this element"),
            (None, Some(_)) => Some("only Beryl 3D has this element"),
            _ if self.b3d_bare * 2 > self.web_pixels => Some("no Beryl 3D element paints here"),
            _ if self.web_bare * 2 > self.b3d_pixels => Some("no web element paints here"),
            _ => None,
        }
    }
}

/// Every differing pixel of a region at least `SPECK` large, attributed to the topmost painting
/// element at its point on each side and so to that element's row. `web_owner` and `b3d_owner`
/// give the element at a point, in whole points from the top left, of `counts` elements on each
/// side; `pairs` maps a web element's index to its Beryl 3D pair's. Returns the rows, worst first, and the pixels no element on
/// either side paints.
pub(crate) fn attribute(
    (label, sizes): (&[u32], &[usize]),
    (width, scale): (u32, f32),
    web_owner: &dyn Fn(u32, u32) -> Option<usize>,
    b3d_owner: &mut dyn FnMut(u32, u32) -> Option<usize>,
    (pairs, counts): (&[(usize, usize)], [usize; 2]),
) -> (Vec<Row>, usize) {
    fn add(rows: &mut Vec<Row>, web: Option<usize>, b3d: Option<usize>) -> usize {
        rows.push(Row {
            web,
            b3d,
            web_pixels: 0,
            b3d_pixels: 0,
            b3d_bare: 0,
            web_bare: 0,
            area: None,
        });
        rows.len() - 1
    }
    let mut rows: Vec<Row> = Vec::new();
    // Each element's row, by its index, once it has one.
    let mut web_row: Vec<Option<usize>> = vec![None; counts[0]];
    let mut b3d_row: Vec<Option<usize>> = vec![None; counts[1]];
    for &(w, b) in pairs {
        let row = add(&mut rows, Some(w), Some(b));
        web_row[w] = Some(row);
        b3d_row[b] = Some(row);
    }
    let mut nobody = 0;
    for (at, &region) in label.iter().enumerate() {
        if region == u32::MAX || sizes[region as usize] < SPECK {
            continue;
        }
        let (x, y) = (at as u32 % width, at as u32 / width);
        let point = ((x as f32 / scale) as u32, (y as f32 / scale) as u32);
        let web = web_owner(point.0, point.1);
        let b3d = b3d_owner(point.0, point.1);
        if web.is_none() && b3d.is_none() {
            nobody += 1;
        }
        let here = Pixels::of_pixel(x, y);
        if let Some(w) = web {
            let row = *web_row[w].get_or_insert_with(|| add(&mut rows, Some(w), None));
            let row = &mut rows[row];
            row.web_pixels += 1;
            row.b3d_bare += usize::from(b3d.is_none());
            row.area = Some(row.area.map_or(here, |area| area.union(here)));
        }
        if let Some(b) = b3d {
            let row = *b3d_row[b].get_or_insert_with(|| add(&mut rows, None, Some(b)));
            let row = &mut rows[row];
            row.b3d_pixels += 1;
            row.web_bare += usize::from(web.is_none());
            row.area = Some(row.area.map_or(here, |area| area.union(here)));
        }
    }
    let mut rows: Vec<Row> = rows.into_iter().filter(|row| row.pixels() > 0).collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.pixels()));
    (rows, nobody)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(rect: [f32; 4], z: i64, paints: bool) -> Record {
        Record {
            label: String::new(),
            rect,
            z,
            paints,
            lines: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn regions_group_pixels_that_touch_along_an_edge() {
        // Two regions: an L of three pixels, and one pixel touching it only at a corner.
        let differ = [
            true, true, false, //
            true, false, false, //
            false, false, true,
        ];
        let (label, sizes) = regions(&differ, 3);
        assert_eq!(sizes, vec![3, 1]);
        assert_eq!(label[0], label[3]);
        assert_ne!(label[0], label[8]);
        assert_eq!(label[4], u32::MAX);
    }

    #[test]
    fn ink_is_what_stands_out_from_the_edge_of_a_line() {
        // A 6 by 4 white box with a black stroke at x 2..4, y 1..3.
        let mut pixels = [255u8; 6 * 4 * 4].to_vec();
        for (x, y) in [(2, 1), (3, 1), (2, 2), (3, 2)] {
            let i = (y * 6 + x) * 4;
            pixels[i..i + 3].fill(0);
        }
        let image = Image {
            pixels: &pixels,
            width: 6,
            height: 4,
        };
        let line = Pixels {
            left: 0,
            top: 0,
            right: 6,
            bottom: 4,
        };
        assert_eq!(
            ink(image, line),
            Some(Pixels {
                left: 2,
                top: 1,
                right: 4,
                bottom: 3
            })
        );
    }

    #[test]
    fn the_topmost_painting_element_owns_a_point() {
        let records = [
            record([0.0, 0.0, 4.0, 4.0], 0, true),
            record([1.0, 1.0, 2.0, 2.0], 2, true),
            // Above the others, but paints nothing, as a layout wrapper.
            record([0.0, 0.0, 4.0, 4.0], 3, false),
        ];
        let grid = owners(&records, 4, 4);
        assert_eq!(grid[0], 0);
        assert_eq!(grid[4 + 1], 1);
    }

    #[test]
    fn a_pair_is_one_row_and_a_bare_side_is_hinted() {
        // Four differing pixels in one region of a 4 by 1 picture at scale 1.
        let label = [0, 0, 0, 0];
        let sizes = [4];
        // Web element 0 paints everywhere; Beryl 3D's element 5, its pair, only at x 0 and 1.
        let web = |_: u32, _: u32| Some(0);
        let mut b3d = |x: u32, _: u32| (x < 2).then_some(5);
        let (rows, nobody) = attribute(
            (&label, &sizes),
            (4, 1.0),
            &web,
            &mut b3d,
            (&[(0, 5)], [1, 6]),
        );
        assert_eq!(nobody, 0);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].web_pixels, rows[0].b3d_pixels), (4, 2));
        assert_eq!(rows[0].pixels(), 4);
        assert_eq!(rows[0].hint(), None, "half of it is drawn by both");

        let mut nothing = |x: u32, _: u32| (x < 1).then_some(5);
        let (rows, _) = attribute(
            (&label, &sizes),
            (4, 1.0),
            &web,
            &mut nothing,
            (&[(0, 5)], [1, 6]),
        );
        assert_eq!(rows[0].hint(), Some("no Beryl 3D element paints here"));
    }

    #[test]
    fn specks_are_not_counted() {
        let label = [0, u32::MAX, 1, 1, 1, 1];
        let sizes = [1, 4];
        let web = |_: u32, _: u32| Some(0);
        let mut b3d = |_: u32, _: u32| None;
        let (rows, _) = attribute((&label, &sizes), (6, 1.0), &web, &mut b3d, (&[], [1, 1]));
        assert_eq!(rows[0].web_pixels, 4);
        assert_eq!(rows[0].hint(), Some("only the web has this element"));
    }
}
