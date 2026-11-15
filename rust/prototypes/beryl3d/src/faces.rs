//! Card faces: every card of the decklist's Scryfall scan, read from the app's own image cache
//! (beryl_app's art.rs, under the Tauri app's cache directory), decoded at startup and scaled to
//! 256 by 360, uncompressed. A scan missing from the cache is downloaded into it first, as the
//! app's own queue does: one image at a time, at least 100 ms apart, as Scryfall's API
//! guidelines ask. A face whose scan cannot be had is a flat colour with a stripe.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

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

/// Where the app's cache keeps the image at `url`.
fn cache_path(cache: &Path, url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix(SCRYFALL_CARDS).or_else(|| {
        url.strip_prefix(SCRYFALL_BACKS)
            .map(|r| r.trim_start_matches('/'))
    })?;
    let path = if url.starts_with(SCRYFALL_BACKS) {
        cache.join("backs").join(rest)
    } else {
        cache.join(rest)
    };
    Some(path)
}

fn cached(cache: &Path, url: &str) -> Option<PathBuf> {
    cache_path(cache, url).filter(|path| path.is_file())
}

/// Downloads each of `urls` the cache lacks, with curl, writing each file atomically.
fn fetch_missing<'a>(cache: &Path, urls: impl IntoIterator<Item = &'a str>) {
    let mut last: Option<Instant> = None;
    for url in urls {
        let Some(path) = cache_path(cache, url) else {
            continue;
        };
        if path.is_file() {
            continue;
        }
        if let Some(previous) = last
            && let Some(wait) = Duration::from_millis(100).checked_sub(previous.elapsed())
        {
            thread::sleep(wait);
        }
        last = Some(Instant::now());
        let Some(folder) = path.parent() else {
            continue;
        };
        let partial = path.with_extension("part");
        let fetched = std::fs::create_dir_all(folder).is_ok()
            && Command::new("curl")
                .args(["--silent", "--fail", "--location", "--max-time", "20"])
                .args([
                    "--user-agent",
                    "ProjectBeryl/0.1",
                    "--header",
                    "Accept: image/jpeg,image/*",
                ])
                .arg("--output")
                .arg(&partial)
                .arg(url)
                .status()
                .is_ok_and(|status| status.success())
            && std::fs::rename(&partial, &path).is_ok();
        if !fetched {
            let _ = std::fs::remove_file(&partial);
            eprintln!("faces: could not download {url}");
        }
    }
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
    // The deck's cards and every token they make.
    let mut names: Vec<String> = deck.iter().map(|(_, name)| name.clone()).collect();
    for (_, name) in &deck {
        if let Some(record) = beryl_card_store::STORE.card(name) {
            names.extend(record.tokens.into_iter().map(|token| token.name));
        }
    }
    let records = card_records_among(names.iter().map(String::as_str));
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
    fetch_missing(
        &cache,
        urls.iter()
            .map(String::as_str)
            .chain([records.card_back.as_str()]),
    );
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
