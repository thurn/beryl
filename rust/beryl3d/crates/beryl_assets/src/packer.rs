//! The atlas's shelf packer (docs/beryl-3d.md, "Text": glyphs are drawn from an atlas). A page
//! is cut into shelves, rows that run the page's width. Every rectangle on a shelf has a height
//! in the shelf's height class, and rectangles are placed along a shelf from left to right. A
//! rectangle opens a new shelf below the last one when no shelf of its class has room, and the
//! page is full when no new shelf fits either.
//!
//! Glyphs have few distinct heights at one size, so equal-height shelves waste little space,
//! and placing one costs a short scan of the shelves.

/// The width and height of an atlas page, in texels.
pub const PAGE_SIZE: u16 = 1024;

/// How many pages the atlas can grow to, 1 MiB each, including the shapes of light on page 0.
pub const ATLAS_PAGES: u16 = 32;

/// Shelf heights are rounded up to a multiple of this, so that glyphs a pixel or two apart in
/// height share a shelf.
const HEIGHT_CLASS: u16 = 4;

/// Empty texels left to the right of and below every rectangle, so that a sampler filtering
/// across a glyph's edge never reads its neighbour.
const GAP: u16 = 1;

/// The page has no room for the rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// One page's shelves.
#[derive(Clone, Debug)]
pub struct Shelves {
    size: u16,
    rows: Vec<Shelf>,
    /// The top of the space below the last shelf.
    bottom: u16,
}

#[derive(Clone, Copy, Debug)]
struct Shelf {
    y: u16,
    height: u16,
    /// The left edge of the free space at the shelf's right end.
    x: u16,
}

impl Shelves {
    /// An empty square page `size` texels wide.
    #[must_use]
    pub fn new(size: u16) -> Self {
        Self {
            size,
            rows: Vec::new(),
            bottom: 0,
        }
    }

    /// Places a `width` by `height` rectangle and returns its top left corner, or [`Full`]
    /// when the page has no room for it. An empty rectangle takes no space and lies at the
    /// origin.
    pub fn insert(&mut self, width: u16, height: u16) -> Result<[u16; 2], Full> {
        if width == 0 || height == 0 {
            return Ok([0, 0]);
        }
        let class = height.div_ceil(HEIGHT_CLASS) * HEIGHT_CLASS;
        let span = u32::from(width) + u32::from(GAP);
        let size = u32::from(self.size);
        if let Some(shelf) = self
            .rows
            .iter_mut()
            .find(|shelf| shelf.height == class && u32::from(shelf.x) + span <= size)
        {
            let corner = [shelf.x, shelf.y];
            shelf.x += span as u16;
            return Ok(corner);
        }
        let rise = u32::from(class) + u32::from(GAP);
        if span > size || u32::from(self.bottom) + rise > size {
            return Err(Full);
        }
        let y = self.bottom;
        self.bottom += rise as u16;
        self.rows.push(Shelf {
            y,
            height: class,
            x: span as u16,
        });
        Ok([0, y])
    }
}
