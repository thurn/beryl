//! The asset workers against scan caches made here: no test reads or assumes the user's own
//! downloaded scans. Cache misses use loopback origins.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use beryl_art::{ArtCache, Origins};
use beryl_assets::{
    CacheKey, Config, ENCODER_VERSION, FaceText, Format, Full, Layer, MatchFace, MatchFaces,
    Prepared, Preparer, Source, Upload, faces_built_on_this_thread, frames_drawn_on_this_thread,
};
use kit_base::atlas::{AssetReady, FaceId, FaceSize};

use crate::test_scans::{Scratch, scan};

/// A card back followed by distinct front faces, in face ID order.
fn test_faces(cards: u16) -> MatchFaces {
    let face = |image: &str, name: String| {
        MatchFace::from_image(
            image,
            name,
            vec!["U".to_owned()],
            FaceText {
                type_line: "Instant".to_owned(),
                ..FaceText::default()
            },
            None,
        )
    };
    let back = face(
        "https://backs.scryfall.io/large/0/a/0a000000-0000-0000-0000-000000000000.jpg",
        "Card back".to_owned(),
    );
    let fronts = (1..=cards).map(|index| {
        face(
            &format!(
                "https://cards.scryfall.io/large/front/a/b/ab000000-0000-0000-0000-{index:012x}.jpg"
            ),
            format!("Example {index}"),
        )
    });
    MatchFaces {
        faces: std::iter::once(back).chain(fronts).collect(),
    }
}

/// Nothing listens here, so a download fails at once.
const NOWHERE: Origins<'static> = Origins {
    cards: "http://127.0.0.1:9",
    backs: "http://127.0.0.1:9",
};

/// Only catches a hang: a face takes milliseconds.
const HANG: Duration = Duration::from_secs(120);

fn preparer(dir: &Path) -> Preparer {
    Preparer::new(Config {
        art: Arc::new(ArtCache::new(dir.join("art"), NOWHERE)),
        faces_dir: dir.join("faces"),
        workers: 2,
        wake: Arc::new(|| {}),
        start: || {},
    })
}

/// Puts a scan in `dir`'s scan cache for every face of the match.
fn cache_every_scan(dir: &Path, faces: &MatchFaces) {
    let bytes = scan();
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
    let faces = test_faces(21);
    cache_every_scan(&scratch.0, &faces);
    let formats = [Format::Etc2Rgba8];

    let mut first = preparer(&scratch.0);
    let results = first.prepare_match(faces.clone(), &formats);
    let count = first.faces().unwrap().faces.len();
    assert!(count > 20, "only {count} faces listed");
    let prepared = receive(&results, count);
    assert!(prepared.iter().all(|p| face_layer(p).1 == Source::Encoded));
    drop(first);

    // The scans go: a face read from anywhere but the compressed cache would be a drawn frame.
    fs::remove_dir_all(scratch.0.join("art")).unwrap();
    let mut second = preparer(&scratch.0);
    let results = second.prepare_match(faces, &formats);
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
                AssetReady::Glyphs { .. } | AssetReady::Icons { .. } => {
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
    let results = assets.prepare_match(test_faces(2), &[Format::Etc2Rgba8]);
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
    let results = assets.prepare_match(test_faces(1), &[Format::Rgba8]);
    let count = assets.faces().unwrap().faces.len();
    // A scan can replace one face while another worker is still drawing a fallback. Check
    // the order for each face and require both uploads to use its board texture layer.
    let prepared = receive(&results, 2 * count);
    for index in 0..count {
        let layer = u16::try_from(index).unwrap();
        let ready = AssetReady::Face {
            face: FaceId(layer),
            size: FaceSize::Board,
            layer,
        };
        let sources: Vec<_> = prepared
            .iter()
            .filter(|prepared| prepared.ready == ready)
            .map(|prepared| face_layer(prepared).1)
            .collect();
        assert_eq!(sources, [Source::Drawn, Source::Encoded], "{ready:?}");
    }
    assert_eq!(faces_built_on_this_thread(), 0);
    assert_eq!(frames_drawn_on_this_thread(), 0);
}

#[test]
fn the_seventeenth_full_size_face_drops_the_least_recently_used() {
    let scratch = Scratch::new("full");
    let mut assets = preparer(&scratch.0);
    let results = assets.prepare_match(test_faces(17), &[Format::Etc2Rgba8]);
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
