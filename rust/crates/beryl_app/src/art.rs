//! Card images, served to the UI over the `beryl-art` URI scheme from a disk cache.
//!
//! `data/cards.json` (written by `by cards`) lists each face's image URLs on Scryfall's image
//! host, such as `https://cards.scryfall.io/art_crop/front/a/b/ab12….jpg?1700000000`, and the
//! card back's on its card back host, `https://backs.scryfall.io/large/0/a/0aee….jpg`. The UI
//! asks for the same path under `beryl-art://localhost/`, a back's under
//! `beryl-art://localhost/backs/`. A hit is read from the cache; a miss
//! goes to a single download queue that fetches one image at a time, at least
//! [`INTERVAL`] apart, as Scryfall's API guidelines ask (<https://scryfall.com/docs/api>), and
//! writes it to the cache atomically. A failure answers with an error status, which the UI shows
//! as a placeholder; the next request for that image tries again.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tauri::http::{Response, StatusCode, header};

pub const SCHEME: &str = "beryl-art";
/// Scryfall's hosts for card images and for card backs.
pub const SCRYFALL: Origins<'static> = Origins {
    cards: "https://cards.scryfall.io",
    backs: "https://backs.scryfall.io",
};
/// The path prefix under which the UI asks for a card back.
const BACKS: &str = "/backs";
/// The least time between two downloads.
pub const INTERVAL: Duration = Duration::from_millis(100);
/// Scryfall's images are a few hundred kilobytes; anything far larger is not one.
const MAX_BYTES: u64 = 8 << 20;
const VERSIONS: [&str; 3] = ["art_crop", "normal", "large"];
const SIDES: [&str; 2] = ["front", "back"];
const BACK_VERSIONS: [&str; 3] = ["small", "normal", "large"];

/// Where images download from: [`SCRYFALL`] outside tests.
#[derive(Debug, Clone, Copy)]
pub struct Origins<'a> {
    pub cards: &'a str,
    pub backs: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Host {
    Cards,
    Backs,
}

/// One image on Scryfall: a card's on its image host, `/<version>/<side>/<id[0]>/<id[1]>/<id>.jpg`,
/// or a card back on its back host, `/<version>/<id[0]>/<id[1]>/<id>.jpg`, asked for under
/// [`BACKS`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct Image {
    host: Host,
    /// The path the UI asks for, which is also the file's in the cache.
    path: String,
    /// The path on its host.
    remote: String,
}

impl Image {
    /// Accepts exactly the paths of Scryfall's images, so a request can neither name another
    /// file on those hosts nor escape the cache directory.
    fn parse(path: &str) -> Option<Self> {
        if let Some(remote) = path.strip_prefix(BACKS) {
            let parts: Vec<&str> = remote.strip_prefix('/')?.split('/').collect();
            let [version, first, second, file] = parts[..] else {
                return None;
            };
            return (BACK_VERSIONS.contains(&version) && named(first, second, file)).then(|| {
                Self {
                    host: Host::Backs,
                    path: path.to_owned(),
                    remote: remote.to_owned(),
                }
            });
        }
        let parts: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
        let [version, side, first, second, file] = parts[..] else {
            return None;
        };
        let ok = VERSIONS.contains(&version) && SIDES.contains(&side) && named(first, second, file);
        ok.then(|| Self {
            host: Host::Cards,
            path: path.to_owned(),
            remote: path.to_owned(),
        })
    }

    fn file(&self, dir: &Path) -> PathBuf {
        dir.join(&self.path[1..])
    }
}

/// Whether `<first>/<second>/<file>` names an image by a Scryfall id, filed under its first two
/// characters: `0/a/0a1b….jpg`.
fn named(first: &str, second: &str, file: &str) -> bool {
    let Some(id) = file.strip_suffix(".jpg") else {
        return false;
    };
    let uuid = id.len() == 36
        && id.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        });
    uuid && first == &id[0..1] && second == &id[1..2]
}

#[derive(Debug)]
pub enum ArtError {
    /// Not the path of a Scryfall image.
    NotAnImage,
    Download(String),
    Io(io::Error),
}

impl fmt::Display for ArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnImage => write!(f, "not a card image path"),
            Self::Download(e) => write!(f, "download failed: {e}"),
            Self::Io(e) => write!(f, "cache: {e}"),
        }
    }
}

struct Job {
    image: Image,
    reply: mpsc::SyncSender<Result<Vec<u8>, ArtError>>,
}

/// The cache and its download queue, whose thread ends when the cache drops.
pub struct ArtCache {
    dir: PathBuf,
    jobs: mpsc::Sender<Job>,
}

impl ArtCache {
    /// A cache in `dir` that downloads misses from `origins`.
    pub fn new(dir: PathBuf, origins: Origins) -> Self {
        let (jobs, queue) = mpsc::channel();
        let downloader = Downloader {
            dir: dir.clone(),
            cards: origins.cards.to_owned(),
            backs: origins.backs.to_owned(),
            agent: agent(),
            last: None,
        };
        thread::Builder::new()
            .name("beryl-art".into())
            .spawn(move || downloader.run(&queue))
            .expect("failed to start the art download thread");
        Self { dir, jobs }
    }

    /// The image at `path`, from the cache or, on a miss, downloaded. Blocks until it has it.
    pub fn get(&self, path: &str) -> Result<Vec<u8>, ArtError> {
        let image = Image::parse(path).ok_or(ArtError::NotAnImage)?;
        if let Ok(bytes) = fs::read(image.file(&self.dir)) {
            return Ok(bytes);
        }
        let (reply, answer) = mpsc::sync_channel(1);
        let stopped = || ArtError::Download("the download queue stopped".into());
        self.jobs
            .send(Job { image, reply })
            .map_err(|_| stopped())?;
        answer.recv().map_err(|_| stopped())?
    }

    /// The `beryl-art` response for a request path.
    pub fn respond(&self, path: &str) -> Response<Vec<u8>> {
        let (status, body) = match self.get(path) {
            Ok(bytes) => (StatusCode::OK, bytes),
            Err(e) => {
                beryl_session::journal::warn(
                    "art",
                    format!("{path}: {e}"),
                    serde_json::Value::Null,
                );
                let status = match e {
                    ArtError::NotAnImage => StatusCode::NOT_FOUND,
                    ArtError::Download(_) | ArtError::Io(_) => StatusCode::BAD_GATEWAY,
                };
                (status, Vec::new())
            }
        };
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "image/jpeg")
            .body(body)
            .expect("a static response is valid")
    }
}

/// The TLS the downloads use: the system's, except on Android (Cargo.toml).
#[cfg(not(target_os = "android"))]
const TLS: ureq::tls::TlsProvider = ureq::tls::TlsProvider::NativeTls;
#[cfg(target_os = "android")]
const TLS: ureq::tls::TlsProvider = ureq::tls::TlsProvider::Rustls;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(format!("ProjectBeryl/{}", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(Duration::from_secs(30)))
        .tls_config(ureq::tls::TlsConfig::builder().provider(TLS).build())
        .build()
        .into()
}

struct Downloader {
    dir: PathBuf,
    cards: String,
    backs: String,
    agent: ureq::Agent,
    last: Option<Instant>,
}

impl Downloader {
    fn run(mut self, queue: &mpsc::Receiver<Job>) {
        for job in queue {
            // The requester may have given up; nothing else needs the answer.
            let _ = job.reply.send(self.fetch(&job.image));
        }
    }

    fn fetch(&mut self, image: &Image) -> Result<Vec<u8>, ArtError> {
        let file = image.file(&self.dir);
        // An earlier job in the queue may have downloaded it already.
        if let Ok(bytes) = fs::read(&file) {
            return Ok(bytes);
        }
        if let Some(wait) = self
            .last
            .and_then(|t| (t + INTERVAL).checked_duration_since(Instant::now()))
        {
            thread::sleep(wait);
        }
        let bytes = self.download(image);
        self.last = Some(Instant::now());
        let bytes = bytes.map_err(|e| ArtError::Download(e.to_string()))?;
        write_atomically(&file, &bytes).map_err(ArtError::Io)?;
        Ok(bytes)
    }

    fn download(&self, image: &Image) -> Result<Vec<u8>, ureq::Error> {
        let origin = match image.host {
            Host::Cards => &self.cards,
            Host::Backs => &self.backs,
        };
        self.agent
            .get(format!("{origin}{}", image.remote))
            .header(header::ACCEPT, "image/jpeg,image/*")
            .call()?
            .body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_vec()
    }
}

/// Writes beside the target, then renames over it, so a reader never sees half an image.
fn write_atomically(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = file.parent().expect("an image file has a directory");
    fs::create_dir_all(dir)?;
    let mut partial = file.as_os_str().to_owned();
    partial.push(format!(".{}.part", std::process::id()));
    let partial = PathBuf::from(partial);
    fs::write(&partial, bytes)?;
    fs::rename(&partial, file).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const ID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    const OTHER: &str = "1a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    fn path(id: &str) -> String {
        format!("/art_crop/front/{}/{}/{id}.jpg", &id[0..1], &id[1..2])
    }

    fn back(id: &str) -> String {
        format!("/large/{}/{}/{id}.jpg", &id[0..1], &id[1..2])
    }

    /// Both hosts at one stub server's origin.
    fn both(origin: &str) -> Origins<'_> {
        Origins {
            cards: origin,
            backs: origin,
        }
    }

    struct Request {
        line: String,
        user_agent: Option<String>,
        at: Instant,
    }

    /// A local HTTP server answering each connection with the next of `answers`, and
    /// reporting each request it gets.
    fn stub(answers: Vec<(u16, &'static [u8])>) -> (String, mpsc::Receiver<Request>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for (status, body) in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut user_agent = None;
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':')
                        && name.eq_ignore_ascii_case("user-agent")
                    {
                        user_agent = Some(value.trim().to_owned());
                    }
                }
                tx.send(Request {
                    line: line.trim().to_owned(),
                    user_agent,
                    at: Instant::now(),
                })
                .unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(body).unwrap();
            }
        });
        (origin, rx)
    }

    /// A fresh directory, removed when the guard drops.
    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn cache_dir() -> (TempDir, PathBuf) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("beryl-art-{}-{n}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        (TempDir(path.clone()), path)
    }

    #[test]
    fn a_hit_is_served_from_disk_without_a_request() {
        let (_dir, dir) = cache_dir();
        let file = dir.join(&path(ID)[1..]);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"cached").unwrap();
        let (origin, requests) = stub(vec![]);

        let cache = ArtCache::new(dir, both(&origin));
        let response = cache.respond(&path(ID));

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"cached");
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn a_miss_downloads_once_and_caches_the_image() {
        let (_dir, dir) = cache_dir();
        let (origin, requests) = stub(vec![(200, b"image")]);
        let cache = ArtCache::new(dir.clone(), both(&origin));

        assert_eq!(cache.get(&path(ID)).unwrap(), b"image");
        assert_eq!(cache.get(&path(ID)).unwrap(), b"image");

        let request = requests.recv().unwrap();
        assert_eq!(request.line, format!("GET {} HTTP/1.1", path(ID)));
        assert_eq!(
            request.user_agent.as_deref(),
            Some(concat!("ProjectBeryl/", env!("CARGO_PKG_VERSION")))
        );
        assert!(requests.try_recv().is_err(), "the second get was a hit");
        assert_eq!(fs::read(dir.join(&path(ID)[1..])).unwrap(), b"image");
    }

    #[test]
    fn a_failed_download_answers_an_error_and_caches_nothing() {
        let (_dir, dir) = cache_dir();
        let (origin, requests) = stub(vec![(404, b"missing")]);
        let cache = ArtCache::new(dir.clone(), both(&origin));

        let response = cache.respond(&path(ID));

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert!(requests.recv().is_ok());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn a_card_back_downloads_from_the_back_host() {
        let (_dir, dir) = cache_dir();
        let (cards, card_requests) = stub(vec![]);
        let (backs, back_requests) = stub(vec![(200, b"back")]);
        let cache = ArtCache::new(
            dir.clone(),
            Origins {
                cards: &cards,
                backs: &backs,
            },
        );
        let asked = format!("/backs{}", back(ID));

        assert_eq!(cache.get(&asked).unwrap(), b"back");

        let request = back_requests.recv().unwrap();
        assert_eq!(request.line, format!("GET {} HTTP/1.1", back(ID)));
        assert!(card_requests.try_recv().is_err());
        assert_eq!(fs::read(dir.join(&asked[1..])).unwrap(), b"back");
    }

    #[test]
    fn downloads_are_spaced_apart() {
        let (_dir, dir) = cache_dir();
        let (origin, requests) = stub(vec![(200, b"one"), (200, b"two")]);
        let cache = ArtCache::new(dir, both(&origin));

        cache.get(&path(ID)).unwrap();
        cache.get(&path(OTHER)).unwrap();

        let first = requests.recv().unwrap().at;
        let second = requests.recv().unwrap().at;
        assert!(second.duration_since(first) >= INTERVAL);
    }

    #[test]
    fn only_scryfall_image_paths_are_served() {
        let (_dir, dir) = cache_dir();
        let (origin, requests) = stub(vec![]);
        let cache = ArtCache::new(dir, both(&origin));
        for bad in [
            "/art_crop/front/1/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/art_crop/front/0/a/../../0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/png/front/0/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/art_crop/left/0/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/art_crop/front/0/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4D.jpg",
            "/art_crop/front/0/a/0a1b2c3d.jpg",
            "/large/0/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/backs/art_crop/0/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/backs/large/0/a/../0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/backs/large/front/0/a/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jpg",
            "/",
        ] {
            assert_eq!(cache.respond(bad).status(), StatusCode::NOT_FOUND, "{bad}");
        }
        assert!(requests.try_recv().is_err());
    }
}
