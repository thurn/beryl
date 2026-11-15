//! The asset workers: threads of this crate's own that read, decode, scale and compress card
//! faces, so that the thread drawing frames never does (docs/beryl-3d.md, "Threads").
//!
//! [`Preparer::prepare_match`] queues every face of a match at board size. [`Preparer::full`]
//! queues one face at full size, into one of [`FULL_LAYERS`] layers that are reused least
//! recently used first. Each finished face arrives on the match's channel as a [`Prepared`],
//! after which the worker calls the host's `wake`.
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

use beryl_art::{ArtCache, Fetch};
use beryl_scene::input::{AssetReady, FaceId, FaceSize};

use crate::cache::{self, CacheKey};
use crate::encode::{ENCODER_VERSION, Format, Layer, decode, encode};
use crate::faces::MatchFaces;
use crate::placeholder::placeholder;
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
    /// Drawn because its scan has not been downloaded. It is not cached: a later match draws
    /// the scan once the download queue has fetched it.
    Placeholder,
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
    /// Starts `config.workers` worker threads (at least one).
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
        let workers = (0..config.workers.max(1))
            .map(|i| {
                let shared = Arc::clone(&shared);
                thread::Builder::new()
                    .name(format!("beryl-assets-{i}"))
                    .spawn(move || work(&shared))
                    .expect("failed to start an asset worker")
            })
            .collect();
        Self {
            jobs: Some(jobs),
            workers,
            shared,
            game: None,
            slots: Slots::default(),
            counter: 0,
        }
    }

    /// Lists every face of `decklists` and their tokens and queues each at board size, in the
    /// first format of `supported` that is compressed (see [`Format::choose`]). Each face
    /// arrives on the returned channel as it is ready, in layer `FaceId.0` of the board-size
    /// array. Jobs of an earlier match still queued are dropped, and the full-size layers
    /// are emptied.
    pub fn prepare_match(
        &mut self,
        decklists: &[&[(u8, &str)]],
        supported: &[Format],
    ) -> Receiver<Prepared> {
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
            faces: MatchFaces::list(decklists),
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
    /// Closes the queue and waits for each worker to finish the face it is on.
    fn drop(&mut self) {
        self.jobs = None;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
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

fn work(shared: &Shared) {
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
        let Some((layer, source)) = build(shared, &job) else {
            continue;
        };
        let ready = AssetReady::Face {
            face: job.face,
            size: job.size,
            layer: job.slot.map_or(job.face.0, |(layer, _)| layer),
        };
        let sent = {
            let _sending = shared.send.lock();
            // The receiver may have been dropped; nobody else wants the face.
            shared.wanted(&job)
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
}

/// The face's layer from the compressed cache, or else made from its scan and cached, or else a
/// placeholder. `None`, with a log line, when the encoder fails.
fn build(shared: &Shared, job: &Job) -> Option<(Layer, Source)> {
    FACES_BUILT.with(|built| built.set(built.get() + 1));
    let face = job.game.faces.get(job.face)?;
    let format = job.game.format;
    let file = CacheKey {
        image: &face.key,
        size: job.size,
        format,
        encoder: ENCODER_VERSION,
    }
    .file(&shared.faces_dir);
    if let Some(layer) = cache::read(&file, format) {
        return Some((layer, Source::Cache));
    }
    // A scan not in the cache is queued for download and the receiver dropped: the queue
    // still fetches it, so the next match has it.
    let scan = match shared.art.request(&face.scan) {
        Ok(Fetch::Cached(path)) => fs::read(path).ok(),
        Ok(Fetch::Queued(_)) | Err(_) => None,
    };
    let decoded = scan.and_then(|bytes| decode(&bytes, job.size).ok());
    let (pixels, source) = if let Some(pixels) = decoded {
        (pixels, Source::Encoded)
    } else {
        let name = face.names.first().map_or("", String::as_str);
        (
            placeholder(name, &face.colors, job.size),
            Source::Placeholder,
        )
    };
    let layer = encode(&pixels, job.size, format)
        .inspect_err(|e| eprintln!("beryl_assets: could not compress {}: {e}", face.key))
        .ok()?;
    if source == Source::Encoded
        && let Err(e) = cache::write(&file, &layer)
    {
        eprintln!("beryl_assets: could not cache {}: {e}", file.display());
    }
    Some((layer, source))
}
