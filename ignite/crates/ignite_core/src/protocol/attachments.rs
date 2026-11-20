//! Image attachments on tickets (see "Attachments" in `docs/ignite.md`): the row the hub keeps
//! for each, the limits a client checks before sending one, and the reading of an image's type
//! and size from its first bytes.
//!
//! The hub stores each file once under its SHA-256 and serves it only on the private listener,
//! since a pasted screenshot can show anything on a person's screen. Only PNG, JPEG, GIF and WebP
//! are accepted, recognized by their bytes and never by a file name. SVG is refused because it
//! can carry script.

use serde::{Deserialize, Serialize};

use crate::ids::TicketId;
use crate::time::Timestamp;

/// The largest file the hub accepts as an attachment, 20 MiB. A full-screen Retina PNG is
/// about 5 to 15 MiB.
pub const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;

/// The total size of the stored files when `[hub] attachment_budget_mb` is unset, in MiB.
pub const DEFAULT_BUDGET_MB: u64 = 2048;

/// The longest attachment name, in characters.
pub const MAX_NAME_CHARS: usize = 100;

/// One attachment of a ticket. Removing the file keeps the row, with `removed_at` set, so the
/// ticket can still say the image was there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub ticket: TicketId,
    /// Unique within the ticket, such as `screenshot.png`.
    pub name: String,
    /// The SHA-256 of the file, in lowercase hexadecimal: its address on the hub.
    pub sha256: String,
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub media_type: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    pub created_at: Timestamp,
    pub created_by: String,
    /// When upkeep removed the file, if it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<Timestamp>,
}

impl Attachment {
    /// One line describing it: `screenshot.png  image/png 2880×1800 4.1 MiB`, or the name and
    /// when the file was removed.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.removed_at {
            Some(at) => format!("{}  removed {}", self.name, at.day()),
            None => format!(
                "{}  {} {}×{} {}",
                self.name,
                self.media_type,
                self.width,
                self.height,
                size_text(self.bytes)
            ),
        }
    }
}

/// A size for people: `812 B`, `14.2 KiB`, `4.1 MiB`.
#[must_use]
pub fn size_text(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)] // A size shown to one decimal place.
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", value / 1024.0)
    } else {
        format!("{:.1} MiB", value / (1024.0 * 1024.0))
    }
}

/// What an image's first bytes say about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub media_type: &'static str,
    /// The file name extension for the type, without the dot.
    pub extension: &'static str,
    pub width: u32,
    pub height: u32,
}

/// The type and size of the image in `bytes`, or `None` when it is not a PNG, JPEG, GIF or WebP
/// whose header can be read.
#[must_use]
pub fn sniff(bytes: &[u8]) -> Option<ImageInfo> {
    png(bytes)
        .or_else(|| gif(bytes))
        .or_else(|| jpeg(bytes))
        .or_else(|| webp(bytes))
}

fn info(media_type: &'static str, extension: &'static str, size: (u32, u32)) -> ImageInfo {
    ImageInfo {
        media_type,
        extension,
        width: size.0,
        height: size.1,
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes(
        bytes.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at + 3)?;
    Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
}

/// The signature, then the `IHDR` chunk, which must come first, holding width and height.
fn png(bytes: &[u8]) -> Option<ImageInfo> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !bytes.starts_with(SIGNATURE) || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    Some(info(
        "image/png",
        "png",
        (be32(bytes, 16)?, be32(bytes, 20)?),
    ))
}

/// `GIF87a` or `GIF89a`, then the logical screen's width and height.
fn gif(bytes: &[u8]) -> Option<ImageInfo> {
    if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    Some(info("image/gif", "gif", (le16(bytes, 6)?, le16(bytes, 8)?)))
}

/// Walks the JPEG's marker segments to the first start-of-frame, which holds the height and
/// width.
fn jpeg(bytes: &[u8]) -> Option<ImageInfo> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    loop {
        // Markers may be padded with extra 0xFF bytes.
        while *bytes.get(at)? == 0xFF && *bytes.get(at + 1)? == 0xFF {
            at += 1;
        }
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        // Markers without a length: TEM and the restart markers.
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            at += 2;
            continue;
        }
        let length = be16(bytes, at + 2)? as usize;
        // Every start-of-frame marker except DHT (C4), JPG (C8) and DAC (CC).
        let frame = (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker);
        if frame {
            let height = be16(bytes, at + 5)?;
            let width = be16(bytes, at + 7)?;
            return Some(info("image/jpeg", "jpg", (width, height)));
        }
        if marker == 0xD9 || marker == 0xDA || length < 2 {
            return None;
        }
        at += 2 + length;
    }
}

/// `RIFF`, a size, `WEBP`, then a `VP8 ` (lossy), `VP8L` (lossless) or `VP8X` (extended) chunk.
fn webp(bytes: &[u8]) -> Option<ImageInfo> {
    if !bytes.starts_with(b"RIFF") || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let size = match bytes.get(12..16)? {
        b"VP8 " => {
            // A key frame's start code, then 14-bit width and height.
            if bytes.get(23..26)? != [0x9D, 0x01, 0x2A] {
                return None;
            }
            (le16(bytes, 26)? & 0x3FFF, le16(bytes, 28)? & 0x3FFF)
        }
        b"VP8L" => {
            if *bytes.get(20)? != 0x2F {
                return None;
            }
            let bits = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1)
        }
        b"VP8X" => (le24(bytes, 24)? + 1, le24(bytes, 27)? + 1),
        _ => return None,
    };
    Some(info("image/webp", "webp", size))
}

/// `name` made safe to store: one path segment of at most [`MAX_NAME_CHARS`] characters, made
/// of letters, digits, `.`, `-` and `_`, with every other character replaced by `-`, not
/// starting with a dot. An empty result is `image`. The extension of the image's type replaces
/// any other, so the name says what the file is.
#[must_use]
pub fn clean_name(name: &str, extension: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => base,
    };
    let mut cleaned: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    while cleaned.starts_with('.') {
        cleaned.remove(0);
    }
    if cleaned.is_empty() {
        "image".clone_into(&mut cleaned);
    }
    let room = MAX_NAME_CHARS.saturating_sub(extension.len() + 1);
    cleaned.truncate(room);
    format!("{cleaned}.{extension}")
}

/// The `n`th name to try when `name` is taken: `shot.png`, then `shot-2.png`, `shot-3.png`.
#[must_use]
pub fn numbered_name(name: &str, n: u32) -> String {
    if n <= 1 {
        return name.to_owned();
    }
    match name.rsplit_once('.') {
        Some((stem, extension)) => format!("{stem}-{n}.{extension}"),
        None => format!("{name}-{n}"),
    }
}

/// The Markdown that shows an attachment in a ticket's text.
#[must_use]
pub fn embed(name: &str, caption: Option<&str>) -> String {
    let alt = caption.filter(|c| !c.trim().is_empty()).unwrap_or(name);
    let alt: String = alt
        .chars()
        .map(|c| {
            if matches!(c, '[' | ']' | '\n') {
                ' '
            } else {
                c
            }
        })
        .collect();
    format!("![{alt}](attachment:{name})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn each_accepted_type_is_recognized_with_its_size() {
        assert_eq!(
            sniff(&png_header(2880, 1800)),
            Some(info("image/png", "png", (2880, 1800)))
        );
        let gif = b"GIF89a\x40\x01\xf0\x00\x00";
        assert_eq!(sniff(gif), Some(info("image/gif", "gif", (320, 240))));
        // SOI, an APP0 segment of 16 bytes, then SOF0 with height 600 and width 800.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend([0; 14]);
        jpeg.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x03, 0x20, 0x03]);
        assert_eq!(sniff(&jpeg), Some(info("image/jpeg", "jpg", (800, 600))));
        let mut lossless = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        // Width 100 and height 50, each stored minus one in 14 bits.
        let bits: u32 = 0x63 | (0x31 << 14);
        lossless.extend(bits.to_le_bytes());
        assert_eq!(
            sniff(&lossless),
            Some(info("image/webp", "webp", (100, 50)))
        );
        let mut extended = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
        extended.extend([0x3F, 0x01, 0x00, 0xC7, 0x00, 0x00]);
        assert_eq!(
            sniff(&extended),
            Some(info("image/webp", "webp", (320, 200)))
        );
    }

    #[test]
    fn text_svg_and_truncated_headers_are_not_images() {
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(sniff(b"hello"), None);
        assert_eq!(sniff(&png_header(1, 1)[..18]), None);
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF]), None);
    }

    #[test]
    fn names_become_one_safe_segment_with_the_type_extension() {
        assert_eq!(clean_name("Screen Shot 1.PNG", "png"), "Screen-Shot-1.png");
        assert_eq!(clean_name("../../etc/passwd", "png"), "passwd.png");
        assert_eq!(clean_name(".hidden.jpg", "jpg"), "hidden.jpg");
        assert_eq!(clean_name("", "gif"), "image.gif");
        assert_eq!(clean_name("photo.jpeg", "png"), "photo.png");
        assert!(clean_name(&"a".repeat(300), "webp").chars().count() <= MAX_NAME_CHARS);
        assert_eq!(numbered_name("shot.png", 1), "shot.png");
        assert_eq!(numbered_name("shot.png", 3), "shot-3.png");
    }

    #[test]
    fn an_embed_uses_the_caption_and_keeps_brackets_out() {
        assert_eq!(embed("a.png", None), "![a.png](attachment:a.png)");
        assert_eq!(
            embed("a.png", Some("the [bad] board")),
            "![the  bad  board](attachment:a.png)"
        );
    }
}
