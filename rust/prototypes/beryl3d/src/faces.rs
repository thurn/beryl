//! Card faces: every card of the decklist's Scryfall scan, read from the app's own image cache
//! (beryl_app's art.rs, under the Tauri app's cache directory), decoded at startup and scaled to
//! 256 by 360, uncompressed. A face with no cached scan is a flat colour with a stripe. Nothing
//! is ever downloaded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread;

use beryl_session::card_records::card_records_among;
use image::imageops::FilterType;

use crate::core::{Faces, LAYER_BACK, LAYER_UNKNOWN};

pub const FACE_W: u32 = 256;
pub const FACE_H: u32 = 360;
const SCRYFALL_CARDS: &str = "https://cards.scryfall.io/";
const SCRYFALL_BACKS: &str = "https://backs.scryfall.io/";

pub struct FaceImages {
    /// One RGBA image per texture layer: the back, the unknown face, then each card face.
    pub layers: Vec<Vec<u8>>,
    pub faces: Faces,
    pub scans: usize,
    pub missing: usize,
}

/// The app's image cache: `~/Library/Caches/ca.thurn.beryl/art` on the Mac.
pub fn cache_dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join("Library/Caches/ca.thurn.beryl/art")
}

fn cached(cache: &Path, url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix(SCRYFALL_CARDS).or_else(|| {
        url.strip_prefix(SCRYFALL_BACKS)
            .map(|r| r.trim_start_matches('/'))
    })?;
    let path = if url.starts_with(SCRYFALL_BACKS) {
        cache.join("backs").join(rest)
    } else {
        cache.join(rest)
    };
    path.is_file().then_some(path)
}

fn placeholder(seed: u32, back: bool) -> Vec<u8> {
    let base = if back {
        [70u8, 45, 30]
    } else {
        [
            (seed * 37 % 150 + 60) as u8,
            (seed * 91 % 150 + 60) as u8,
            (seed * 53 % 150 + 60) as u8,
        ]
    };
    let mut pixels = vec![0u8; (FACE_W * FACE_H * 4) as usize];
    for y in 0..FACE_H {
        for x in 0..FACE_W {
            let border = x < 10 || x >= FACE_W - 10 || y < 10 || y >= FACE_H - 10;
            let stripe = (40..64).contains(&y);
            let c = if border {
                [20, 20, 20]
            } else if stripe {
                [240, 235, 220]
            } else {
                base
            };
            let o = ((y * FACE_W + x) * 4) as usize;
            pixels[o..o + 3].copy_from_slice(&c);
            pixels[o + 3] = 255;
        }
    }
    pixels
}

fn decode(path: &Path) -> Option<Vec<u8>> {
    let image = image::open(path).ok()?;
    Some(
        image
            .resize_exact(FACE_W, FACE_H, FilterType::Triangle)
            .to_rgba8()
            .into_raw(),
    )
}

/// Loads every face of the default decklist, decoding scans on all cores.
pub fn load() -> FaceImages {
    let cache = cache_dir();
    let deck = beryl_cards::decks::default_deck();
    let records = card_records_among(deck.iter().map(|(_, name)| name.as_str()));
    // Each distinct image, with the names that show it.
    let mut images: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in records.cards.values().chain(records.tokens.values()) {
        for (i, face) in entry.faces.iter().enumerate() {
            let names = images.entry(face.images.normal.clone()).or_default();
            names.push(face.name.clone());
            if i == 0 {
                names.push(entry.name.clone());
            }
        }
    }
    let urls: Vec<String> = images.keys().cloned().collect();
    let decoded: Vec<Option<Vec<u8>>> = thread::scope(|scope| {
        let chunk = urls.len().div_ceil(8).max(1);
        let handles: Vec<_> = urls
            .chunks(chunk)
            .map(|chunk| {
                let cache = &cache;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|url| cached(cache, url).and_then(|p| decode(&p)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    let back = cached(&cache, &records.card_back).and_then(|p| decode(&p));
    let mut layers = vec![
        back.unwrap_or_else(|| placeholder(0, true)),
        placeholder(1, false),
    ];
    let mut faces = Faces::default();
    let (mut scans, mut missing) = (0, 0);
    for (i, (url, image)) in urls.iter().zip(decoded).enumerate() {
        let layer = layers.len() as u16;
        match image {
            Some(pixels) => {
                scans += 1;
                layers.push(pixels);
            }
            None => {
                missing += 1;
                layers.push(placeholder(i as u32 + 2, false));
            }
        }
        for name in &images[url] {
            faces.by_name.insert(name.clone(), layer);
        }
    }
    faces.layers = (layers.len() - 2) as u16;
    debug_assert_eq!(LAYER_BACK, 0);
    debug_assert_eq!(LAYER_UNKNOWN, 1);
    FaceImages {
        layers,
        faces,
        scans,
        missing,
    }
}
