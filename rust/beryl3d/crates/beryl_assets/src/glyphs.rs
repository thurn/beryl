//! The glyph rasterizer (docs/beryl-3d.md, "Text": glyphs are drawn from an atlas; "Hosts":
//! `AssetReady::Glyphs`). A [`GlyphWorker`] owns a thread that rasterizes each glyph the host
//! asks for with `swash`, once, into an 8-bit coverage bitmap, packs it into a single-channel
//! atlas page and sends each new or changed page as a [`Prepared`]. The caller's thread only
//! sends keys, so the frame thread never rasterizes.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};

use beryl_scene::input::{AssetReady, GlyphCell, GlyphKey};
use beryl_scene::text::fonts::ALL;
use beryl_scene::text::glyph_key;
use beryl_scene::tokens::type_style;
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

use crate::packer::{Full, PAGE_SIZE, Shelves};
use crate::{Prepared, Upload};

/// The atlas page the worker fills first. Page 0 holds the shapes of light ([`crate::light`]).
pub const FIRST_PAGE: u16 = 1;

/// The keys of the printable ASCII characters, space to tilde, of every type style in
/// `beryl_scene::tokens` at `scale` physical pixels per point, for a host to hand to
/// [`GlyphWorker::request`] before its first frame. A character a font lacks is left out.
#[must_use]
pub fn prepare_latin(scale: f32) -> Vec<GlyphKey> {
    let mut keys = Vec::new();
    for style in type_style::ALL {
        let Some(font) = FontRef::from_index(style.font.file().data, 0) else {
            continue;
        };
        let charmap = font.charmap();
        for character in ' '..='~' {
            let glyph = charmap.map(character);
            if glyph != 0 {
                keys.push(glyph_key(style, scale, glyph));
            }
        }
    }
    keys
}

thread_local! {
    static GLYPHS_RASTERIZED: Cell<u64> = const { Cell::new(0) };
}

/// How many glyphs this thread has rasterized. Only the glyph worker rasterizes, so on any
/// other thread, such as the one that draws frames, this stays zero.
#[must_use]
pub fn glyphs_rasterized_on_this_thread() -> u64 {
    GLYPHS_RASTERIZED.with(Cell::get)
}

/// A thread that rasterizes glyphs. Dropping it stops the thread once it has finished what it
/// was asked.
#[derive(Debug)]
pub struct GlyphWorker {
    requests: Option<Sender<Message>>,
    thread: Option<JoinHandle<()>>,
}

impl GlyphWorker {
    /// Starts the thread, which first calls `start` to set its scheduling class (`beryl_host`'s
    /// `asset_thread`), since it would otherwise inherit its maker's. It sends each finished
    /// page to `out` and then calls `wake`, so that a host whose event loop is idle draws the
    /// new glyphs at once.
    #[must_use]
    pub fn spawn(out: Sender<Prepared>, wake: impl Fn() + Send + 'static, start: fn()) -> Self {
        let (requests, inbox) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("beryl-glyphs".into())
            .spawn(move || {
                start();
                run(&inbox, &out, &wake);
            })
            .expect("the glyph worker's thread starts");
        Self {
            requests: Some(requests),
            thread: Some(thread),
        }
    }

    /// Asks for glyphs. A glyph asked for before, in this call or an earlier one, is not
    /// rasterized or sent again.
    pub fn request(&self, keys: impl IntoIterator<Item = GlyphKey>) {
        let keys: Vec<GlyphKey> = keys.into_iter().collect();
        if keys.is_empty() {
            return;
        }
        if let Some(requests) = &self.requests {
            // The worker only stops when this sender is dropped, or when the host has dropped
            // the receiver of finished pages and wants none.
            let _ = requests.send(Message::Keys(keys));
        }
    }

    /// Waits until every glyph asked for so far has been rasterized and its page sent, so
    /// that a host drawing a picture of one moment, such as the headless host, can take every
    /// page it needs before it draws. A host drawing frames as they come never calls it.
    pub fn flush(&self) {
        let Some(requests) = &self.requests else {
            return;
        };
        let (done, wait) = mpsc::sync_channel(1);
        if requests.send(Message::Flush(done)).is_ok() {
            // An error means the worker stopped, and has nothing left to send.
            let _ = wait.recv();
        }
    }
}

/// What a host sends the worker.
#[derive(Debug)]
enum Message {
    /// Glyphs to rasterize.
    Keys(Vec<GlyphKey>),
    /// Answer once every glyph asked for before this has been sent.
    Flush(SyncSender<()>),
}

impl Drop for GlyphWorker {
    fn drop(&mut self) {
        self.requests = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The worker's loop: takes every request waiting, rasterizes them together so that a burst
/// of requests uploads each page once, sends the pages, and then answers the flushes that were
/// waiting with them.
fn run(inbox: &Receiver<Message>, out: &Sender<Prepared>, wake: &dyn Fn()) {
    let mut atlas = Atlas::new();
    while let Ok(first) = inbox.recv() {
        let mut keys = Vec::new();
        let mut flushes = Vec::new();
        for message in std::iter::once(first).chain(inbox.try_iter()) {
            match message {
                Message::Keys(more) => keys.extend(more),
                Message::Flush(done) => flushes.push(done),
            }
        }
        let prepared = atlas.add(&keys);
        let sent = !prepared.is_empty();
        for page in prepared {
            if out.send(page).is_err() {
                return;
            }
        }
        if sent {
            wake();
        }
        for done in flushes {
            let _ = done.send(());
        }
    }
}

/// The atlas's pages: every glyph rasterized so far and the page being filled.
struct Atlas {
    context: ScaleContext,
    done: BTreeSet<GlyphKey>,
    /// The number of the page being filled.
    page: u16,
    pixels: Vec<u8>,
    shelves: Shelves,
}

impl Atlas {
    fn new() -> Self {
        Self {
            context: ScaleContext::new(),
            done: BTreeSet::new(),
            page: FIRST_PAGE,
            pixels: blank_page(),
            shelves: Shelves::new(PAGE_SIZE),
        }
    }

    /// Rasterizes each glyph of `keys` not rasterized before and returns the pages that
    /// changed, each with the glyphs added to it. A page that fills up is sent and a new one
    /// started. A glyph whose outline is empty, such as a space, is sent with an empty
    /// rectangle. A glyph too large for a page, or of a font that does not exist, is never
    /// sent, and draws nothing.
    fn add(&mut self, keys: &[GlyphKey]) -> Vec<Prepared> {
        let mut prepared = Vec::new();
        let mut cells = Vec::new();
        for &key in keys {
            if !self.done.insert(key) {
                continue;
            }
            let Some(bitmap) = self.rasterize(key) else {
                continue;
            };
            let (width, height) = (bitmap.width, bitmap.height);
            let placed = self.shelves.insert(width, height).or_else(|Full| {
                prepared.push(self.finish_page(std::mem::take(&mut cells)));
                self.shelves.insert(width, height)
            });
            let Ok(corner) = placed else {
                continue;
            };
            self.blit(&bitmap, corner);
            cells.push(GlyphCell {
                key,
                rect: [corner[0], corner[1], bitmap.width, bitmap.height],
                bearing: bitmap.bearing,
            });
        }
        if !cells.is_empty() {
            prepared.push(Prepared {
                upload: Upload::AtlasPage {
                    page: self.page,
                    pixels: self.pixels.clone(),
                },
                ready: AssetReady::Glyphs {
                    page: self.page,
                    glyphs: cells,
                },
            });
        }
        prepared
    }

    /// Hands over the full page with `cells`, its glyphs not yet sent, and starts the next.
    fn finish_page(&mut self, cells: Vec<GlyphCell>) -> Prepared {
        let page = self.page;
        self.page += 1;
        self.shelves = Shelves::new(PAGE_SIZE);
        Prepared {
            upload: Upload::AtlasPage {
                page,
                pixels: std::mem::replace(&mut self.pixels, blank_page()),
            },
            ready: AssetReady::Glyphs {
                page,
                glyphs: cells,
            },
        }
    }

    /// Draws the glyph `key` as coverage, without hinting, with the variable font's weight
    /// axis set to the key's weight.
    fn rasterize(&mut self, key: GlyphKey) -> Option<Bitmap> {
        GLYPHS_RASTERIZED.with(|count| count.set(count.get() + 1));
        let file = ALL.get(usize::from(key.font))?;
        let font = FontRef::from_index(file.data, 0)?;
        let mut scaler = self
            .context
            .builder(font)
            .size(f32::from(key.size))
            .hint(false)
            .variations([("wght", f32::from(key.weight))])
            .build();
        let image = Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, key.glyph)?;
        let placement = image.placement;
        // The packer leaves a texel's gap beside every glyph, so a glyph as wide as a page has no room.
        let too_large = |length: u32| length >= u32::from(PAGE_SIZE);
        if too_large(placement.width) || too_large(placement.height) {
            return None;
        }
        Some(Bitmap {
            width: placement.width as u16,
            height: placement.height as u16,
            // swash measures the top edge up from the baseline; a cell's bearing is y down.
            bearing: [
                saturate(placement.left),
                saturate(placement.top.saturating_neg()),
            ],
            coverage: image.data,
        })
    }

    /// Copies `bitmap` into the page with its top left texel at `corner`.
    fn blit(&mut self, bitmap: &Bitmap, corner: [u16; 2]) {
        let width = usize::from(bitmap.width);
        if width == 0 {
            return;
        }
        let stride = usize::from(PAGE_SIZE);
        for (row, line) in bitmap.coverage.chunks_exact(width).enumerate() {
            let start = (usize::from(corner[1]) + row) * stride + usize::from(corner[0]);
            self.pixels[start..start + width].copy_from_slice(line);
        }
    }
}

/// One rasterized glyph.
struct Bitmap {
    width: u16,
    height: u16,
    bearing: [i16; 2],
    /// `height` rows of `width` coverage bytes.
    coverage: Vec<u8>,
}

fn blank_page() -> Vec<u8> {
    vec![0; usize::from(PAGE_SIZE) * usize::from(PAGE_SIZE)]
}

fn saturate(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
