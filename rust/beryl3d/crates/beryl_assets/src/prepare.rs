//! The asset workers: threads of this crate's own that read, decode, scale and compress card
//! faces, so that the thread drawing frames never does (docs/beryl-3d.md, "Threads").
//!
//! [`Preparer::prepare_match`] queues every face of a match at board size. [`Preparer::full`]
//! queues one face at full size, into one of [`FULL_LAYERS`] layers that are reused least
//! recently used first. Each finished face arrives on the match's channel as a [`Prepared`],
//! after which the worker calls the host's `wake`.
//!
//! A face whose scan has not been downloaded is drawn as Beryl's own card frame, and its scan
//! is queued for download. One more thread waits for those downloads in the order they were
//! queued and, as each scan lands, sends the face again made from its scan, under the same
//! layer. The host holds such a face until the client is still (`Renderer::hold`), so that a
//! card never changes its picture in the middle of a flight.
//!
//! The thread that calls the `Preparer` only lists faces and sends jobs down a channel; it takes
//! no lock that a worker takes. Workers share the job queue's lock among themselves.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use beryl_art::{ArtCache, ArtError, Fetch};
use beryl_scene::faces::FaceList;
use beryl_scene::input::{AssetReady, FaceId, FaceSize};

use crate::cache::{self, CacheKey};
use crate::encode::{ENCODER_VERSION, Format, Layer, decode, encode};
use crate::faces::{CARD_BACK, MatchFace, MatchFaces};
use crate::frame::{card_back, frame, plate_over};
use crate::{Prepared, Upload};

/// How many full-size faces are kept: the layers of the full-size texture array.
pub const FULL_LAYERS: usize = 16;

/// What the workers need from their host.
pub struct Config {
    /// The scan cache and its download queue.
    pub art: Arc<ArtCache>,
    /// The compressed faces' cache directory, beside the scan cache's.
    pub faces_dir: PathBuf,
    /// How many worker threads to start; [`default_workers`] outside tests.
    pub workers: usize,
    /// Called by a worker after it sends a face, so that a host whose event loop is idle draws
    /// the face at once.
    pub wake: Arc<dyn Fn() + Send + Sync>,
    /// Called first on each worker thread, to set its scheduling class (`beryl_host`'s
    /// `asset_thread`). A thread otherwise inherits the class of the one that made it, and a
    /// preparer made on the frame thread would compress faces at drawing's class.
    pub start: fn(),
}

/// Two to four workers, as the machine has cores.
#[must_use]
pub fn default_workers() -> usize {
    thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 4))
}

/// Where a face came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Read from the compressed cache.
    Cache,
    /// Decoded from its scan and compressed, then written to the compressed cache.
    Encoded,
    /// Drawn as Beryl's card frame because its scan has not been downloaded. It is not cached:
    /// the face is sent again from its scan when the download lands, and a later match reads
    /// the scan.
    Drawn,
}

/// The answer to [`Preparer::full`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Full {
    /// The face already has this full-size layer, or one is being prepared for it.
    Kept { layer: u16 },
    /// The face is queued for this layer, which `evicted`'s full-size face held until now: from
    /// here on that face is drawn at board size.
    Queued { layer: u16, evicted: Option<FaceId> },
}

thread_local! {
    static FACES_BUILT: Cell<u64> = const { Cell::new(0) };
}

/// How many faces this thread has read from a cache, decoded or compressed. Only the workers
/// do that work, so on any other thread, such as the one that draws frames, this stays zero.
#[must_use]
pub fn faces_built_on_this_thread() -> u64 {
    FACES_BUILT.with(Cell::get)
}

/// The faces and format of one match, shared by its jobs.
struct Match {
    number: u64,
    faces: MatchFaces,
    format: Format,
    results: Sender<Prepared>,
}

/// Where the download queue answers with a scan's file, or the failure.
type Download = Receiver<Result<PathBuf, ArtError>>;

/// A face drawn as Beryl's frame, waiting for its scan's download.
struct Late {
    job: Job,
    answer: Download,
}

struct Job {
    game: Arc<Match>,
    face: FaceId,
    size: FaceSize,
    /// For a full-size face, its layer and the number of the assignment that gave it that layer.
    slot: Option<(u16, u64)>,
}

/// What the workers share.
struct Shared {
    jobs: Mutex<Receiver<Job>>,
    art: Arc<ArtCache>,
    faces_dir: PathBuf,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The number of the match being prepared. Jobs of an earlier match are dropped.
    current: AtomicU64,
    /// For each full-size layer, the number of the assignment that holds it. A finished face
    /// whose assignment has been replaced is dropped.
    slots: [AtomicU64; FULL_LAYERS],
    /// Held by a worker while it checks that its result is still wanted and sends it, so that
    /// two results for one layer arrive in the order the layer was assigned.
    send: Mutex<()>,
}

/// The full-size layers' owners, kept on the caller's thread.
#[derive(Default)]
struct Slots {
    faces: [Option<FaceId>; FULL_LAYERS],
    /// When each layer was last asked for, on the `tick` clock.
    used: [u64; FULL_LAYERS],
    tick: u64,
}

/// The asset workers and the full-size layers they fill.
pub struct Preparer {
    jobs: Option<Sender<Job>>,
    workers: Vec<JoinHandle<()>>,
    shared: Arc<Shared>,
    game: Option<Arc<Match>>,
    slots: Slots,
    /// Numbers matches and full-size assignments, so that a stale job can be told apart.
    counter: u64,
}

impl Preparer {
    /// Starts `config.workers` worker threads (at least one), and the thread that waits for
    /// late scans.
    #[must_use]
    pub fn new(config: Config) -> Self {
        let (jobs, queue) = mpsc::channel();
        let shared = Arc::new(Shared {
            jobs: Mutex::new(queue),
            art: config.art,
            faces_dir: config.faces_dir,
            wake: config.wake,
            current: AtomicU64::new(0),
            slots: std::array::from_fn(|_| AtomicU64::new(0)),
            send: Mutex::new(()),
        });
        let start = config.start;
        let (late, waiting) = mpsc::channel();
        let workers = (0..config.workers.max(1))
            .map(|i| {
                let shared = Arc::clone(&shared);
                let late = late.clone();
                thread::Builder::new()
                    .name(format!("beryl-assets-{i}"))
                    .spawn(move || {
                        start();
                        work(&shared, &late);
                    })
                    .expect("failed to start an asset worker")
            })
            .collect();
        // Not joined: it may be waiting on a download when the preparer drops. It ends once the
        // workers have, after dropping what it still holds as no longer wanted.
        let watcher = Arc::clone(&shared);
        thread::Builder::new()
            .name("beryl-assets-late".to_owned())
            .spawn(move || {
                start();
                watch(&watcher, &waiting);
            })
            .expect("failed to start the late scan thread");
        Self {
            jobs: Some(jobs),
            workers,
            shared,
            game: None,
            slots: Slots::default(),
            counter: 0,
        }
    }

    /// Queues every face of `list` at board size, in the first format of `supported` that is
    /// compressed (see [`Format::choose`]). `list` is the client's (`Client::faces`), so that
    /// each face arrives on the returned channel as it is ready, in layer `FaceId.0` of the
    /// board-size array, under the `FaceId` the client knows it by. Jobs of an earlier match
    /// still queued are dropped, and the full-size layers are emptied.
    pub fn prepare_match(&mut self, list: &FaceList, supported: &[Format]) -> Receiver<Prepared> {
        let format = Format::choose(supported);
        if format == Format::Rgba8 {
            eprintln!(
                "beryl_assets: the GPU has none of ASTC, ETC2 and BC7, so card faces are \
                 uncompressed and take four times the memory"
            );
        }
        self.counter += 1;
        self.shared.current.store(self.counter, Ordering::SeqCst);
        self.slots = Slots::default();
        let (results, received) = mpsc::channel();
        let game = Arc::new(Match {
            number: self.counter,
            faces: MatchFaces::new(list),
            format,
            results,
        });
        for at in 0..game.faces.faces.len() {
            self.send(Job {
                game: Arc::clone(&game),
                face: FaceId(at as u16),
                size: FaceSize::Board,
                slot: None,
            });
        }
        self.game = Some(game);
        received
    }

    /// The faces of the match being prepared, which give each [`FaceId`] its picture.
    #[must_use]
    pub fn faces(&self) -> Option<&MatchFaces> {
        self.game.as_deref().map(|game| &game.faces)
    }

    /// Asks for `face` at full size. A face that already has a full-size layer keeps it;
    /// otherwise the least recently asked-for layer is reused and the face is queued for it.
    /// `None` when no match is being prepared or it has no such face.
    pub fn full(&mut self, face: FaceId) -> Option<Full> {
        let game = Arc::clone(self.game.as_ref()?);
        game.faces.get(face)?;
        let slots = &mut self.slots;
        slots.tick += 1;
        if let Some(at) = slots.faces.iter().position(|&f| f == Some(face)) {
            slots.used[at] = slots.tick;
            return Some(Full::Kept { layer: at as u16 });
        }
        let at = (0..FULL_LAYERS)
            .min_by_key(|&at| (slots.faces[at].is_some(), slots.used[at]))
            .unwrap_or(0);
        let evicted = slots.faces[at].replace(face);
        slots.used[at] = slots.tick;
        self.counter += 1;
        self.shared.slots[at].store(self.counter, Ordering::SeqCst);
        let layer = at as u16;
        self.send(Job {
            game,
            face,
            size: FaceSize::Full,
            slot: Some((layer, self.counter)),
        });
        Some(Full::Queued { layer, evicted })
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = &self.jobs {
            // The workers end only when `jobs` drops, so a send cannot fail.
            let _ = jobs.send(job);
        }
    }
}

impl Drop for Preparer {
    /// Closes the queue and waits for each worker to finish the faces queued. Faces still
    /// waiting for their scans are dropped.
    fn drop(&mut self) {
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        self.shared.current.store(0, Ordering::SeqCst);
    }
}

impl Shared {
    fn wanted(&self, job: &Job) -> bool {
        let current = self.current.load(Ordering::SeqCst) == job.game.number;
        let slot = job.slot.is_none_or(|(layer, assignment)| {
            self.slots[usize::from(layer)].load(Ordering::SeqCst) == assignment
        });
        current && slot
    }
}

fn work(shared: &Shared, late: &Sender<Late>) {
    loop {
        let job = {
            let Ok(queue) = shared.jobs.lock() else {
                return;
            };
            let Ok(job) = queue.recv() else {
                return;
            };
            job
        };
        if !shared.wanted(&job) {
            continue;
        }
        let Some((layer, source, answer)) = build(shared, &job) else {
            continue;
        };
        deliver(shared, &job, layer, source);
        if let Some(answer) = answer {
            // The late thread ends only after every worker, so a send cannot fail.
            let _ = late.send(Late { job, answer });
        }
    }
}

/// Sends a finished face on its match's channel, if it is still wanted, and wakes the host.
fn deliver(shared: &Shared, job: &Job, layer: Layer, source: Source) {
    let ready = AssetReady::Face {
        face: job.face,
        size: job.size,
        layer: job.slot.map_or(job.face.0, |(layer, _)| layer),
    };
    let sent = {
        let _sending = shared.send.lock();
        // The receiver may have been dropped; nobody else wants the face.
        shared.wanted(job)
            && job
                .game
                .results
                .send(Prepared {
                    upload: Upload::Face { layer, source },
                    ready,
                })
                .is_ok()
    };
    if sent {
        (shared.wake)();
    }
}

/// Waits for each drawn face's scan, in the order the download queue fetches them, and sends
/// the face again made from its scan. A face no longer wanted (its match or its full-size layer
/// replaced) is dropped without waiting.
fn watch(shared: &Shared, waiting: &Receiver<Late>) {
    for Late { job, answer } in waiting {
        if !shared.wanted(&job) {
            continue;
        }
        let Ok(Ok(path)) = answer.recv() else {
            continue;
        };
        if !shared.wanted(&job) {
            continue;
        }
        let Some(face) = job.game.faces.get(job.face) else {
            continue;
        };
        FACES_BUILT.with(|built| built.set(built.get() + 1));
        let Some(pixels) = fs::read(path)
            .ok()
            .and_then(|bytes| from_scan(&bytes, face, job.size))
        else {
            continue;
        };
        let file = cache_file(shared, &job, face);
        if let Some(layer) = compress(&pixels, &job, face, Some(&file)) {
            deliver(shared, &job, layer, Source::Encoded);
        }
    }
}

/// Where the face of `job` is kept in the compressed cache.
fn cache_file(shared: &Shared, job: &Job, face: &MatchFace) -> PathBuf {
    CacheKey {
        image: &face.key,
        size: job.size,
        format: job.game.format,
        encoder: ENCODER_VERSION,
    }
    .file(&shared.faces_dir)
}

/// The face's pixels from its scan's bytes: the scan scaled to the face's size, and for a face
/// that shows an ability on the stack, its plate drawn over it.
fn from_scan(bytes: &[u8], face: &MatchFace, size: FaceSize) -> Option<Vec<u8>> {
    let pixels = decode(bytes, size).ok()?;
    Some(match &face.plate {
        Some(plate) => plate_over(pixels, plate, size),
        None => pixels,
    })
}

/// The face's pixels compressed with their mipmaps and, when `file` is given, written to the
/// compressed cache. `None`, with a log line, when the encoder fails.
fn compress(pixels: &[u8], job: &Job, face: &MatchFace, file: Option<&PathBuf>) -> Option<Layer> {
    let layer = encode(pixels, job.size, job.game.format)
        .inspect_err(|e| eprintln!("beryl_assets: could not compress {}: {e}", face.key))
        .ok()?;
    if let Some(file) = file
        && let Err(e) = cache::write(file, &layer)
    {
        eprintln!("beryl_assets: could not cache {}: {e}", file.display());
    }
    Some(layer)
}

/// The face's layer from the compressed cache, or else made from its scan and cached, or else
/// drawn as Beryl's frame (the card back as the web's plain back), with the receiver of its
/// scan's download when one was queued.
/// `None`, with a log line, when the encoder fails.
fn build(shared: &Shared, job: &Job) -> Option<(Layer, Source, Option<Download>)> {
    FACES_BUILT.with(|built| built.set(built.get() + 1));
    let face = job.game.faces.get(job.face)?;
    let format = job.game.format;
    let file = cache_file(shared, job, face);
    if let Some(layer) = cache::read(&file, format) {
        return Some((layer, Source::Cache, None));
    }
    let (scan, answer) = match shared.art.request(&face.scan) {
        Ok(Fetch::Cached(path)) => (fs::read(path).ok(), None),
        Ok(Fetch::Queued(answer)) => (None, Some(answer)),
        Err(_) => (None, None),
    };
    if let Some(pixels) = scan.and_then(|bytes| from_scan(&bytes, face, job.size)) {
        let layer = compress(&pixels, job, face, Some(&file))?;
        return Some((layer, Source::Encoded, None));
    }
    let drawn = if job.face == CARD_BACK {
        card_back(job.size)
    } else {
        frame(face, job.size)
    };
    let layer = compress(&drawn, job, face, None)?;
    Some((layer, Source::Drawn, answer))
}
