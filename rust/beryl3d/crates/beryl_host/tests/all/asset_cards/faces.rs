//! The asset workers against scan caches made here: no test reads or assumes the user's own
//! downloaded scans, and downloads go to an address that refuses them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use beryl_art::{ArtCache, Origins};
use beryl_assets::{
    CacheKey, Config, ENCODER_VERSION, Format, Full, Layer, Prepared, Preparer, Source, Upload,
    faces_built_on_this_thread, frames_drawn_on_this_thread,
};
use kit_ui::input::{AssetReady, FaceId, FaceSize};

use super::decklist_faces;

/// The main deck of the WU Control sample list, the Milestone 1 mirror, as `(count, name)`
/// lines: read from the file rather than from `beryl_cards`, so that these tests link no cards.
fn wu_control() -> Vec<(u8, &'static str)> {
    include_str!(
        "../../../../../../../docs/sample-decks/2025-11-18-17lands-cb8d5d5f-wu-control.txt"
    )
    .lines()
    .take_while(|line| !line.trim().is_empty())
    .map(|line| {
        let (count, name) = line.split_once(' ').expect("a line is a count and a name");
        (count.parse().expect("a count is a number"), name.trim())
    })
    .collect()
}

/// Nothing listens here, so a download fails at once.
const NOWHERE: Origins<'static> = Origins {
    cards: "http://127.0.0.1:9",
    backs: "http://127.0.0.1:9",
};

/// Only catches a hang: a face takes milliseconds.
const HANG: Duration = Duration::from_secs(120);

/// A directory of its own for one test, emptied first and removed when dropped.
pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("beryl_assets-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn preparer(dir: &Path) -> Preparer {
    Preparer::new(Config {
        art: Arc::new(ArtCache::new(dir.join("art"), NOWHERE)),
        faces_dir: dir.join("faces"),
        workers: 2,
        wake: Arc::new(|| {}),
        start: || {},
    })
}

/// A plain 672 by 936 JPEG, the size of Scryfall's large scans.
pub(super) fn scan() -> Vec<u8> {
    let picture = image::RgbImage::from_pixel(672, 936, image::Rgb([40, 90, 160]));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&picture)
        .unwrap();
    bytes
}

/// Puts a scan in `dir`'s scan cache for every face of the WU Control list.
fn cache_every_scan(dir: &Path) {
    let bytes = scan();
    let faces = beryl_assets::MatchFaces::new(decklist_faces(&[&wu_control()]).faces());
    for face in &faces.faces {
        let file = dir.join("art").join(&face.scan[1..]);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, &bytes).unwrap();
    }
}

/// A face's layer and where it came from.
fn face_layer(prepared: &Prepared) -> (&Layer, Source) {
    match &prepared.upload {
        Upload::Face { layer, source } => (layer, *source),
        Upload::AtlasPage { .. } => panic!("a face worker sent a glyph page"),
    }
}

/// Receives `count` faces.
fn receive(results: &Receiver<Prepared>, count: usize) -> Vec<Prepared> {
    (0..count)
        .map(|_| results.recv_timeout(HANG).unwrap())
        .collect()
}

#[test]
fn the_cache_key_changes_with_the_encoder_version() {
    let key = |encoder| CacheKey {
        image: "0aeebaf5-8c7d-4636-9e82-8c27447861f7-front",
        size: FaceSize::Board,
        format: Format::Astc4x4,
        encoder,
    };
    let dir = Path::new("/faces");
    assert_ne!(
        key(ENCODER_VERSION).file(dir),
        key(ENCODER_VERSION + 1).file(dir)
    );
}

#[test]
fn a_second_match_reads_every_face_from_the_cache_on_worker_threads() {
    let scratch = Scratch::new("second");
    cache_every_scan(&scratch.0);
    let faces = decklist_faces(&[&wu_control(), &wu_control()]);
    let formats = [Format::Etc2Rgba8];

    let mut first = preparer(&scratch.0);
    let results = first.prepare_match(faces.faces(), &formats);
    let count = first.faces().unwrap().faces.len();
    assert!(count > 20, "only {count} faces listed");
    let prepared = receive(&results, count);
    assert!(prepared.iter().all(|p| face_layer(p).1 == Source::Encoded));
    drop(first);

    // The scans go: a face read from anywhere but the compressed cache would be a drawn frame.
    fs::remove_dir_all(scratch.0.join("art")).unwrap();
    let mut second = preparer(&scratch.0);
    let results = second.prepare_match(faces.faces(), &formats);
    let again = receive(&results, count);
    assert!(again.iter().all(|p| face_layer(p).1 == Source::Cache));

    let layers = |faces: &[Prepared]| -> BTreeMap<u16, _> {
        faces
            .iter()
            .map(|p| match p.ready {
                AssetReady::Face { layer, size, .. } => {
                    assert_eq!(size, FaceSize::Board);
                    (layer, face_layer(p).0.clone())
                }
                AssetReady::Glyphs { .. }
                | AssetReady::Icons { .. }
                | AssetReady::GalleryFace { .. } => {
                    panic!("a face worker sent an atlas page")
                }
            })
            .collect()
    };
    assert_eq!(layers(&prepared), layers(&again));
    assert_eq!(layers(&again).len(), count);
    assert_eq!(faces_built_on_this_thread(), 0);
}

#[test]
fn a_face_with_no_scan_is_drawn_and_is_not_cached() {
    let scratch = Scratch::new("drawn");
    let mut assets = preparer(&scratch.0);
    let results = assets.prepare_match(
        decklist_faces(&[&wu_control()]).faces(),
        &[Format::Etc2Rgba8],
    );
    let count = assets.faces().unwrap().faces.len();
    let prepared = receive(&results, count);
    for prepared in &prepared {
        let (layer, source) = face_layer(prepared);
        assert_eq!(source, Source::Drawn);
        assert_eq!((layer.width, layer.height), (256, 360));
        assert_eq!(layer.format, Format::Etc2Rgba8);
        // A 256 by 360 face halves to 1 by 1 in nine levels; 8 bits a pixel at the top.
        assert_eq!(layer.levels.len(), 9);
        assert_eq!(layer.levels[0].len(), 256 * 360);
    }
    assert!(!scratch.0.join("faces").exists());
    assert_eq!(faces_built_on_this_thread(), 0);
    assert_eq!(frames_drawn_on_this_thread(), 0);
}

/// An address that answers every request with `body`, until the test ends.
fn serve(body: Vec<u8>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    address
}

#[test]
fn a_drawn_face_is_sent_again_from_its_scan_once_the_scan_downloads() {
    let scratch = Scratch::new("late");
    let origin = serve(scan());
    let origins = Origins {
        cards: &origin,
        backs: &origin,
    };
    let mut assets = Preparer::new(Config {
        art: Arc::new(ArtCache::new(scratch.0.join("art"), origins)),
        faces_dir: scratch.0.join("faces"),
        workers: 2,
        wake: Arc::new(|| {}),
        start: || {},
    });
    let results = assets.prepare_match(
        decklist_faces(&[&[(1, "Island")]]).faces(),
        &[Format::Rgba8],
    );
    let count = assets.faces().unwrap().faces.len();
    // Every face is drawn first, since no scan is cached, and then sent again, under the same
    // face and layer, once the download queue has fetched its scan. The renderer holds such a
    // face until no card is moving (`faces_that_arrive_during_play_wait_for_a_still_client` in
    // beryl_render).
    let drawn = receive(&results, count);
    assert!(drawn.iter().all(|p| face_layer(p).1 == Source::Drawn));
    let late = receive(&results, count);
    assert!(late.iter().all(|p| face_layer(p).1 == Source::Encoded));
    let mut first: Vec<_> = drawn.iter().map(|p| p.ready.clone()).collect();
    let mut again: Vec<_> = late.iter().map(|p| p.ready.clone()).collect();
    first.sort_by_key(|ready| format!("{ready:?}"));
    again.sort_by_key(|ready| format!("{ready:?}"));
    assert_eq!(first, again);
    assert_eq!(faces_built_on_this_thread(), 0);
    assert_eq!(frames_drawn_on_this_thread(), 0);
}

#[test]
fn the_seventeenth_full_size_face_drops_the_least_recently_used() {
    let scratch = Scratch::new("full");
    let mut assets = preparer(&scratch.0);
    let results = assets.prepare_match(
        decklist_faces(&[&wu_control()]).faces(),
        &[Format::Etc2Rgba8],
    );
    let board = assets.faces().unwrap().faces.len();
    assert!(board > 17);
    for face in 1..=16 {
        assert_eq!(
            assets.full(FaceId(face)),
            Some(Full::Queued {
                layer: face - 1,
                evicted: None
            })
        );
    }
    // Face 1 is read again, so face 2 is now the least recently used.
    assert_eq!(assets.full(FaceId(1)), Some(Full::Kept { layer: 0 }));
    assert_eq!(
        assets.full(FaceId(17)),
        Some(Full::Queued {
            layer: 1,
            evicted: Some(FaceId(2))
        })
    );

    // Face 2's job may finish before face 17 takes its layer, or be dropped as stale, so the
    // number of results is not fixed: receive until every board face has come and layer 1
    // holds face 17. A result for face 2 after face 17's would leave layer 1 wrong.
    let (mut boards, mut full) = (0, BTreeMap::new());
    while boards < board || full.len() < 16 || full.get(&1) != Some(&FaceId(17)) {
        let prepared = results.recv_timeout(HANG).unwrap();
        match prepared.ready {
            AssetReady::Face {
                face,
                size: FaceSize::Full,
                layer,
            } => {
                let (picture, _) = face_layer(&prepared);
                assert_eq!((picture.width, picture.height), (672, 936));
                full.insert(layer, face);
            }
            _ => boards += 1,
        }
    }
    assert_eq!(faces_built_on_this_thread(), 0);
}
