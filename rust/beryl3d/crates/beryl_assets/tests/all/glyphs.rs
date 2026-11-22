//! The glyph rasterizer, through its worker: what a host sends and what comes back.

use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use beryl_assets::glyphs::{
    FIRST_PAGE, GlyphWorker, IconSet, glyphs_rasterized_on_this_thread, prepare_latin,
};
use beryl_assets::packer::PAGE_SIZE;
use beryl_assets::{Prepared, Upload};
use kit_base::atlas::{AssetReady, GlyphCell, GlyphKey};
use kit_text::text::fonts::Font;
use swash::FontRef;

/// Only catches a hung worker; a page normally arrives in milliseconds.
const HANG: Duration = Duration::from_mins(1);

fn open_sans(character: char, size: u16) -> GlyphKey {
    let font = FontRef::from_index(Font::OpenSans.file().data, 0).unwrap();
    GlyphKey {
        blur: 0,
        font: Font::OpenSans as u8,
        weight: 400,
        glyph: font.charmap().map(character),
        size,
    }
}

/// The next page the worker sends, with its pixels and its glyphs.
fn next(results: &Receiver<Prepared>) -> (Vec<u8>, Vec<GlyphCell>) {
    let prepared = results.recv_timeout(HANG).expect("the worker sends a page");
    let Upload::AtlasPage { page, pixels } = prepared.upload else {
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
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
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
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
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
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
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
    // Page 0 holds the shapes of light, so glyphs start on the page after it.
    let numbers = FIRST_PAGE..FIRST_PAGE + pages.len() as u16;
    assert_eq!(pages, numbers.collect::<Vec<_>>());
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
        kit_base::tokens::type_style::ALL.len() * printable,
        "each bundled text font has all of ASCII, at every type style's size"
    );
    let title = kit_base::tokens::type_style::TITLE;
    assert!(keys.iter().any(
        |key| key.font == title.font.face(title.weight, title.italic)
            && key.weight == title.weight
            && key.size == 52
    ));
}

#[test]
fn a_space_is_sent_with_an_empty_rectangle() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
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
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
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

#[test]
fn a_rescale_batch_is_rasterized_off_the_thread_that_asks() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
    worker.request(prepare_latin(1.0));
    // The display's scale changes: every glyph is asked for again at the new size. Asking only
    // sends the keys to the worker, so the thread that draws frames rasterizes nothing.
    worker.request(prepare_latin(2.0));
    assert_eq!(glyphs_rasterized_on_this_thread(), 0);
    worker.flush();
    assert_eq!(glyphs_rasterized_on_this_thread(), 0);
    assert!(results.try_iter().count() > 0);
}

#[test]
fn a_mana_symbol_is_inked() {
    let (out, results) = mpsc::channel();
    let worker = GlyphWorker::spawn(out, || {}, || {}, IconSet::default());
    let font = FontRef::from_index(Font::Symbols.file().data, 0).expect("the Mana font parses");
    let glyph = font.charmap().map('\u{e601}');
    assert_ne!(glyph, 0, "the Mana font has the blue symbol");
    let key = GlyphKey {
        blur: 0,
        font: Font::Symbols as u8,
        weight: 400,
        glyph,
        size: 32,
    };
    worker.request([key]);
    let (_, cells) = next(&results);
    assert_eq!(cells[0].key, key);
    assert!(
        cells[0].rect[2] > 0 && cells[0].rect[3] > 0,
        "{:?}",
        cells[0]
    );
}
