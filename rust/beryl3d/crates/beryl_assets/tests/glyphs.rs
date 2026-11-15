//! The glyph rasterizer, through its worker: what a host sends and what comes back.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use beryl_assets::glyphs::{GlyphWorker, prepare_latin};
use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};
use beryl_scene::input::{AssetReady, GlyphCell, GlyphKey};
use beryl_scene::text::fonts::Font;
use swash::FontRef;

/// Only catches a hung worker; a page normally arrives in milliseconds.
const HANG: Duration = Duration::from_mins(1);

fn open_sans(character: char, size: u16) -> GlyphKey {
    let font = FontRef::from_index(Font::OpenSans.file().data, 0).unwrap();
    GlyphKey {
        font: Font::OpenSans as u8,
        weight: 400,
        glyph: font.charmap().map(character),
        size,
    }
}

/// The next page the worker sends, with its pixels and its glyphs.
fn next(results: &Receiver<Prepared>) -> (Vec<u8>, Vec<GlyphCell>) {
    let prepared = results.recv_timeout(HANG).expect("the worker sends a page");
    let Upload::GlyphPage { page, pixels } = prepared.upload else {
        panic!("the glyph worker sent a face");
    };
    let AssetReady::Glyphs {
        page: ready_page,
        glyphs,
    } = prepared.ready
    else {
        panic!("a glyph worker sends glyphs");
    };
    assert_eq!(page, ready_page);
    assert_eq!(
        pixels.len(),
        usize::from(PAGE_SIZE) * usize::from(PAGE_SIZE)
    );
    (pixels, glyphs)
}

/// The coverage of texel `(x, y)` of `cell`'s rectangle.
fn coverage(pixels: &[u8], cell: &GlyphCell, x: u16, y: u16) -> u8 {
    let [left, top, ..] = cell.rect;
    pixels[usize::from(top + y) * usize::from(PAGE_SIZE) + usize::from(left + x)]
}

#[test]
fn a_glyph_asked_for_twice_is_rasterized_once() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {});
    let p = open_sans('P', 28);
    let q = open_sans('Q', 28);
    worker.request([p, p]);
    let (_, first) = next(&results);
    assert_eq!(first.iter().map(|cell| cell.key).collect::<Vec<_>>(), [p]);
    worker.request([p, q]);
    let (_, second) = next(&results);
    assert_eq!(second.iter().map(|cell| cell.key).collect::<Vec<_>>(), [q]);
    worker.request([p]);
    drop(worker);
    assert!(
        results.try_recv().is_err(),
        "nothing new, so no page is sent"
    );
}

#[test]
fn the_capital_p_is_inked_in_its_stem_and_empty_in_its_bowl() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {});
    worker.request([open_sans('P', 28)]);
    let (pixels, cells) = next(&results);
    let [cell] = cells[..] else {
        panic!("one glyph was asked for");
    };
    let [.., width, height] = cell.rect;
    assert!(width > 8 && height > 16, "a 28-pixel capital: {cell:?}");
    assert!(cell.bearing[1] < 0, "a capital rises above the baseline");
    // The stem runs down the left side, below the bowl; the bowl's counter is the empty
    // middle of the upper half.
    let stem = coverage(&pixels, &cell, width * 3 / 20, height * 3 / 4);
    let bowl = coverage(&pixels, &cell, width / 2, height / 4);
    assert!(stem > 200, "stem coverage {stem}");
    assert_eq!(bowl, 0, "bowl coverage");
}

#[test]
fn glyphs_that_overflow_a_page_go_on_to_the_next() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {});
    let capitals: Vec<GlyphKey> = ('A'..='Z').map(|c| open_sans(c, 400)).collect();
    worker.request(capitals.clone());
    drop(worker);
    let mut sent = Vec::new();
    let mut pages = Vec::new();
    for prepared in &results {
        let AssetReady::Glyphs { page, glyphs } = prepared.ready else {
            panic!("a glyph worker sends glyphs");
        };
        pages.push(page);
        sent.extend(glyphs.iter().map(|cell| cell.key));
    }
    assert!(
        pages.len() > 1,
        "26 capitals at 400 pixels fill a page: {pages:?}"
    );
    assert_eq!(pages, (0..pages.len() as u16).collect::<Vec<_>>());
    assert_eq!(
        sent, capitals,
        "each glyph is sent once, in the order asked"
    );
}

#[test]
fn latin_glyphs_cover_every_type_style() {
    let keys = prepare_latin(2.0);
    let printable = usize::from(b'~' - b' ') + 1;
    assert_eq!(
        keys.len(),
        3 * printable,
        "each bundled text font has all of ASCII"
    );
    let title = beryl_scene::tokens::type_style::TITLE;
    assert!(
        keys.iter().any(|key| key.font == title.font as u8
            && key.weight == title.weight
            && key.size == 52)
    );
}

#[test]
fn a_space_is_sent_with_an_empty_rectangle() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {});
    let space = open_sans(' ', 28);
    worker.request([space]);
    let (_, cells) = next(&results);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].key, space);
    assert_eq!(cells[0].rect[2..], [0, 0]);
}

#[test]
fn a_flush_returns_once_every_page_asked_for_is_sent() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {});
    worker.request(prepare_latin(1.0));
    worker.flush();
    let pages: Vec<Prepared> = results.try_iter().collect();
    assert!(
        !pages.is_empty(),
        "no page had arrived when the flush returned"
    );
    // Asking again for glyphs already sent sends nothing, and the flush still returns.
    worker.request(prepare_latin(1.0));
    worker.flush();
    assert_eq!(results.try_iter().count(), 0);
}
