//! The atlas's shelf packer.

use beryl_assets::packer::{Full, PAGE_SIZE, Shelves};

type Rect = [u16; 4];

fn overlap(a: Rect, b: Rect) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// Fills a page with rectangles of assorted glyph-like sizes until it reports full, and
/// returns the rectangles placed.
fn fill() -> Vec<Rect> {
    let mut shelves = Shelves::new(PAGE_SIZE);
    let mut placed = Vec::new();
    for index in 0u32.. {
        let width = (5 + index * 7 % 37) as u16;
        let height = (8 + index * 11 % 29) as u16;
        match shelves.insert(width, height) {
            Ok([x, y]) => placed.push([x, y, width, height]),
            Err(Full) => break,
        }
    }
    placed
}

#[test]
fn rectangles_never_overlap_and_stay_on_the_page() {
    let placed = fill();
    assert!(
        placed.len() > 500,
        "a page holds many glyphs: {}",
        placed.len()
    );
    for (index, &a) in placed.iter().enumerate() {
        assert!(
            a[0] + a[2] <= PAGE_SIZE && a[1] + a[3] <= PAGE_SIZE,
            "{a:?}"
        );
        for &b in &placed[index + 1..] {
            assert!(!overlap(a, b), "{a:?} overlaps {b:?}");
        }
    }
}

#[test]
fn a_full_page_is_reported() {
    let mut shelves = Shelves::new(64);
    assert_eq!(shelves.insert(65, 4), Err(Full), "wider than the page");
    for _ in 0..4 {
        shelves
            .insert(60, 12)
            .expect("four 13-texel shelves fit in 64");
    }
    assert_eq!(shelves.insert(60, 12), Err(Full));
    assert!(
        shelves.insert(2, 12).is_ok(),
        "a shelf of the same height has room left"
    );
    assert_eq!(shelves.insert(2, 30), Err(Full), "no room for a new shelf");
}

#[test]
fn an_empty_rectangle_takes_no_space() {
    let mut shelves = Shelves::new(16);
    assert_eq!(shelves.insert(0, 12), Ok([0, 0]));
    assert_eq!(shelves.insert(15, 12), Ok([0, 0]));
}
