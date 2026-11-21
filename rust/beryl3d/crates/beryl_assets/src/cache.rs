//! The disk cache of compressed faces, which sits beside the scan cache (docs/beryl-3d.md, "Card
//! faces", "Preparation"). A face is filed by its picture's identity, its size, its format and
//! the encoder's version, so a hit is one file read and a change to the encoder makes every face
//! again.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kit_base::atlas::FaceSize;

use crate::encode::{Format, Layer};

/// The first bytes of every cached face.
const MAGIC: &[u8; 8] = b"BERYLFC1";

/// Where one compressed face is filed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheKey<'a> {
    /// The picture's identity, a `MatchFace`'s `key`: its Scryfall id and side.
    pub image: &'a str,
    pub size: FaceSize,
    pub format: Format,
    /// The [`ENCODER_VERSION`](crate::ENCODER_VERSION) that made it.
    pub encoder: u32,
}

impl CacheKey<'_> {
    /// The face's file under the cache directory `dir`:
    /// `<dir>/v<encoder>/<format>/<size>/<image>.face`.
    #[must_use]
    pub fn file(&self, dir: &Path) -> PathBuf {
        let size = match self.size {
            FaceSize::Board => "board",
            FaceSize::Full => "full",
        };
        dir.join(format!("v{}", self.encoder))
            .join(self.format.name())
            .join(size)
            .join(format!("{}.face", self.image))
    }
}

/// The cached face filed under `file`, or `None` when there is none or it is not a whole face
/// in `format` (a file cut short, say): then the face is made again.
pub fn read(file: &Path, format: Format) -> Option<Layer> {
    let bytes = fs::read(file).ok()?;
    parse(&bytes).filter(|layer| layer.format == format)
}

/// Files `layer` under `file`, through a temporary file renamed into place, so that a reader
/// never sees half a face and two workers writing the same face both leave a whole one.
pub fn write(file: &Path, layer: &Layer) -> io::Result<()> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut bytes = Vec::with_capacity(layer.bytes() + 64);
    bytes.extend_from_slice(MAGIC);
    bytes.push(layer.format.code());
    bytes.extend_from_slice(&layer.width.to_le_bytes());
    bytes.extend_from_slice(&layer.height.to_le_bytes());
    bytes.push(layer.levels.len() as u8);
    for level in &layer.levels {
        bytes.extend_from_slice(&(level.len() as u32).to_le_bytes());
        bytes.extend_from_slice(level);
    }
    // Named for the process and thread, so two writers never share a temporary file.
    let thread = format!("{:?}", std::thread::current().id());
    let thread: String = thread.chars().filter(char::is_ascii_digit).collect();
    let partial = file.with_extension(format!("part-{}-{thread}", std::process::id()));
    fs::write(&partial, &bytes)?;
    fs::rename(&partial, file).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

fn parse(bytes: &[u8]) -> Option<Layer> {
    let mut rest = bytes.strip_prefix(MAGIC)?;
    let mut take = |n: usize| -> Option<&[u8]> {
        let (head, tail) = rest.split_at_checked(n)?;
        rest = tail;
        Some(head)
    };
    let format = Format::from_code(take(1)?[0])?;
    let width = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let height = u32::from_le_bytes(take(4)?.try_into().ok()?);
    let count = take(1)?[0];
    let mut levels = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let len = u32::from_le_bytes(take(4)?.try_into().ok()?);
        levels.push(take(len as usize)?.to_vec());
    }
    (rest.is_empty() && !levels.is_empty()).then_some(Layer {
        format,
        width,
        height,
        levels,
    })
}
