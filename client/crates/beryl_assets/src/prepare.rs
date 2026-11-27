//! The asset workers: threads of this crate's own that read, decode, scale and compress card
//! faces, so that the thread drawing frames never does (docs/beryl-3d.md, "Threads").
//!
//! [`Preparer::prepare_match`] queues every face of a match at board size. [`Preparer::full`]
//! queues one face at full size, into one of [`FULL_LAYERS`] layers that are reused least
//! recently used first, those whose faces are not on screen before those whose faces are. Each
//! finished face arrives on the match's channel as a [`Prepared`], after which the worker calls
//! the host's `wake`.
//!
//! A face whose scan has not been downloaded is drawn as Beryl's own card frame, and its scan
//! is queued for download. One more thread waits for those downloads in the order they were
//! queued and, as each scan lands, sends the face again made from its scan, under the same
//! layer. The host holds such a face until the client is still (`Renderer::hold`), so that a
//! card never changes its picture in the middle of a flight.
//!
//! [`Preparer::placeholders`] asks a worker for the placeholders the board-size array shows
//! until each face arrives, so that the frame thread only copies them.
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
use kit_base::atlas::{AssetReady, FaceId, FaceSize};

use crate::cache::{self, CacheKey};
use crate::encode::{ENCODER_VERSION, Format, Layer, decode, encode, encode_cutout};
use crate::faces::{MatchFace, MatchFaces};
use crate::frame::{card_back, frame, plate_over};
use crate::placeholder::Placeholders;
use crate::{Prepared, Upload};

/// The host supplies the card back first.
const CARD_BACK: FaceId = FaceId(0);

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

/// What a worker is asked to do.
enum Work {
    /// Prepare one face.
    Face(Job),
    /// Draw the placeholders of faces with these names and send them on `answer`.
    Placeholders {
        names: Vec<String>,
        format: Format,
        answer: Sender<Placeholders>,
    },
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
    jobs: Mutex<Receiver<Work>>,
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
    /// The faces the host says are on screen at full size (`Preparer::keep`).
    kept: Vec<FaceId>,
}

/// The asset workers and the full-size layers they fill.
pub struct Preparer {
    jobs: Option<Sender<Work>>,
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

    /// Queues every face at board size, in the first compressed format in `supported`
    /// (see [`Format::choose`]). The host supplies the faces in client order, with the card
    /// back first. Each face arrives on the returned channel in layer `FaceId.0` of the
    /// board-size array. Jobs still queued for an earlier match are dropped, and the
    /// full-size layers are emptied.
    pub fn prepare_match(&mut self, faces: MatchFaces, supported: &[Format]) -> Receiver<Prepared> {
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
            faces,
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

    /// Asks a worker for the board-size placeholders of faces named `names`, in `FaceId` order
    /// with the card back first, already in `format`'s mipmap levels. They arrive once, on the
    /// returned channel, after which the worker calls the host's `wake`. A new list replaces the
    /// match's arrays, so jobs still queued for the match before are dropped, as
    /// `prepare_match` would drop them, rather than drawn ahead of the placeholders.
    pub fn placeholders(&mut self, names: Vec<String>, format: Format) -> Receiver<Placeholders> {
        self.counter += 1;
        self.shared.current.store(self.counter, Ordering::SeqCst);
        let (answer, received) = mpsc::channel();
        self.send_work(Work::Placeholders {
            names,
            format,
            answer,
        });
        received
    }

    /// The faces of the match being prepared, which give each [`FaceId`] its picture.
    #[must_use]
    pub fn faces(&self) -> Option<&MatchFaces> {
        self.game.as_deref().map(|game| &game.faces)
    }

    /// Asks for `face` at full size. A face that already has a full-size layer keeps it;
    /// otherwise the least recently asked-for layer whose face is not on screen (`keep`) is
    /// reused, and the face is queued for it.
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
        // An empty layer first, then the least recently asked-for layer whose face is not on
        // screen. A hand or stack card is asked for once, when it appears, so by age alone a
        // card the player hovered later would push it, or the card back every full-size card
        // needs, out of its layer while it is still shown.
        let at = (0..FULL_LAYERS)
            .min_by_key(|&at| {
                let face = slots.faces[at];
                let kept = face.is_some_and(|face| slots.kept.contains(&face));
                (face.is_some(), kept, slots.used[at])
            })
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

    /// Records `faces` as the ones on screen at full size, in place of those recorded before.
    /// `full` reuses the layer of one of them only when every layer holds one.
    pub fn keep(&mut self, faces: &[FaceId]) {
        self.slots.kept.clear();
        self.slots.kept.extend_from_slice(faces);
    }

    /// Drops full-size assignments when the renderer frees their array. On return no
    /// worker can send another delivery from the old assignments.
    pub fn forget_full(&mut self) {
        let _sending = self.shared.send.lock();
        for slot in &self.shared.slots {
            slot.store(0, Ordering::SeqCst);
        }
        self.slots = Slots::default();
    }

    fn send(&self, job: Job) {
        self.send_work(Work::Face(job));
    }

    fn send_work(&self, work: Work) {
        if let Some(jobs) = &self.jobs {
            // The workers end only when `jobs` drops, so a send cannot fail.
            let _ = jobs.send(work);
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
        let work = {
            let Ok(queue) = shared.jobs.lock() else {
                return;
            };
            let Ok(work) = queue.recv() else {
                return;
            };
            work
        };
        let job = match work {
            Work::Face(job) => job,
            Work::Placeholders {
                names,
                format,
                answer,
            } => {
                // The receiver may have been dropped by a newer list; nobody else wants them.
                if answer.send(Placeholders::draw(&names, format)).is_ok() {
                    (shared.wake)();
                }
                continue;
            }
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
    Some(plate_over(pixels, face, size))
}

/// The face's pixels compressed with their mipmaps and, when `file` is given, written to the
/// compressed cache. `None`, with a log line, when the encoder fails.
fn compress(pixels: &[u8], job: &Job, face: &MatchFace, file: Option<&PathBuf>) -> Option<Layer> {
    let encode_face = if face.plate.as_ref().is_some_and(|plate| plate.choice) {
        encode_cutout
    } else {
        encode
    };
    let layer = encode_face(pixels, job.size, job.game.format)
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
