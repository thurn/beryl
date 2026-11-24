//! A private scan cache and a synthetic JPEG for asset worker and host tests.

use std::fs;
use std::path::PathBuf;

/// A directory of its own for one test, emptied first and removed when dropped.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Self {
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

/// A plain 672 by 936 JPEG, the size of Scryfall's large scans.
pub(crate) fn scan() -> Vec<u8> {
    let picture = image::RgbImage::from_pixel(672, 936, image::Rgb([40, 90, 160]));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&picture)
        .unwrap();
    bytes
}
