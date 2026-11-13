//! The window as it is drawn now, for a bug report's frozen frame: `WKWebView`'s own snapshot
//! of the page (`takeSnapshotWithConfiguration:completionHandler:`), which, unlike a screen
//! capture, needs no screen-recording permission and holds only the app's pixels, at the
//! display's scale. On Windows, `WebView2`'s (`CapturePreview`), as a PNG, which is what it
//! makes and what Chromium shows. On phones, the native mobile plugin captures a PNG.
//!
//! The pixels stay here. [`capture`] keeps the platform's image bytes and hands the UI a number.
//! The UI displays them through the art scheme ([`Snapshots::respond`]). [`Snapshots::pngs`]
//! decodes the frame and crops the image for the report.
//! Encoding a retina window to PNG and sending it through IPC as base64, for the UI to decode
//! and encode again, held the reporter for seconds.

use std::io::Cursor;
#[cfg(any(target_os = "macos", windows))]
use std::sync::mpsc::channel;
use std::sync::{Mutex, MutexGuard, PoisonError};
#[cfg(any(target_os = "macos", windows, mobile))]
use std::time::Duration;

use beryl_session::bugs::{Images, Region};
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Manager};

/// How long a snapshot may take before the report goes on without one.
#[cfg(any(target_os = "macos", windows, mobile))]
const TIMEOUT: Duration = Duration::from_secs(5);
/// The path the snapshots are served under, on the `beryl-art` scheme.
pub const PATH: &str = "/snapshot/";

/// The latest snapshot: the reporter freezes one frame at a time.
#[derive(Default)]
pub struct Snapshots(Mutex<Option<(u32, Vec<u8>)>>);

impl Snapshots {
    fn lock(&self) -> MutexGuard<'_, Option<(u32, Vec<u8>)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Serves snapshot `<n>` at `/snapshot/<n>`, as a TIFF on macOS and a PNG elsewhere.
    pub fn respond(&self, path: &str) -> Response<Vec<u8>> {
        let wanted = path.strip_prefix(PATH).and_then(|n| n.parse::<u32>().ok());
        let found = self
            .lock()
            .as_ref()
            .filter(|(id, _)| Some(*id) == wanted)
            .map(|(_, image)| image.clone());
        match found {
            Some(image) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", platform::CONTENT_TYPE)
                .header("Cache-Control", "no-store")
                .body(image),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new()),
        }
        .expect("a snapshot response is valid")
    }

    /// Snapshot `id` as PNGs: the whole window, and `region` of it (in CSS pixels of a page
    /// `page_width` wide), if given.
    pub fn pngs(&self, id: u32, region: Option<Region>, page_width: f64) -> Result<Images, String> {
        let image = self
            .lock()
            .as_ref()
            .filter(|(n, _)| *n == id)
            .map(|(_, image)| image.clone())
            .ok_or_else(|| format!("no snapshot {id}"))?;
        let pixels = platform::pixels(&image)?;
        let screenshot = match region {
            Some(region) => Some(encode(&pixels.crop(region, page_width))?),
            None => None,
        };
        Ok(Images {
            screenshot,
            window: Some(encode(&pixels)?),
        })
    }
}

/// Takes a snapshot of the main window's page and keeps it, as the latest; returns its number.
pub async fn capture(app: &AppHandle) -> Result<u32, String> {
    let image = capture_image(app).await?;
    let snapshots = app.state::<Snapshots>();
    let mut latest = snapshots.lock();
    let id = latest.as_ref().map_or(1, |(n, _)| n + 1);
    *latest = Some((id, image));
    Ok(id)
}

#[cfg(any(target_os = "macos", windows))]
async fn capture_image(app: &AppHandle) -> Result<Vec<u8>, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let (send, receive) = channel::<Result<Vec<u8>, String>>();
    window
        .with_webview(move |webview| platform::snapshot(&webview, send))
        .map_err(|e| e.to_string())?;
    // The snapshot completes on the main thread, which this must not block.
    tauri::async_runtime::spawn_blocking(move || {
        receive
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|e| Err(format!("no snapshot: {e}")))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(mobile)]
async fn capture_image(app: &AppHandle) -> Result<Vec<u8>, String> {
    let app = app.clone();
    // Finish reading and deleting the file even if the caller times out.
    let snapshot = tauri::async_runtime::spawn(async move { platform::snapshot(&app).await });
    tokio::time::timeout(TIMEOUT, snapshot)
        .await
        .map_err(|_| "the WebView snapshot timed out".to_owned())?
        .map_err(|e| e.to_string())?
}

#[cfg(not(any(target_os = "macos", windows, mobile)))]
async fn capture_image(_: &AppHandle) -> Result<Vec<u8>, String> {
    Err(platform::UNAVAILABLE.to_owned())
}

/// An image as 8-bit RGBA, row by row.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    /// `region`, in CSS pixels of a page `page_width` wide, whose top left these pixels share.
    fn crop(&self, region: Region, page_width: f64) -> Pixels {
        let scale = f64::from(self.width) / page_width;
        let clamp = |v: f64, max: u32| (v * scale).round().clamp(0.0, f64::from(max)) as u32;
        let (x0, y0) = (clamp(region.x, self.width), clamp(region.y, self.height));
        let x1 = clamp(region.x + region.width, self.width)
            .max(x0 + 1)
            .min(self.width);
        let y1 = clamp(region.y + region.height, self.height)
            .max(y0 + 1)
            .min(self.height);
        let (width, height) = (x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1));
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in y0..y0 + height {
            let start = ((y * self.width + x0) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + (width * 4) as usize]);
        }
        Pixels {
            width,
            height,
            rgba,
        }
    }
}

fn encode(pixels: &Pixels) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = png::Encoder::new(&mut out, pixels.width, pixels.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&pixels.rgba)
        .map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Objective-C calls into WebKit and AppKit.
#[path = "snapshot/macos.rs"]
mod platform;

#[cfg(windows)]
#[allow(unsafe_code)] // COM calls into WebView2.
#[path = "snapshot/windows.rs"]
mod platform;

#[cfg(mobile)]
#[path = "snapshot/mobile.rs"]
mod platform;

#[cfg(not(any(target_os = "macos", windows, mobile)))]
mod platform {
    use super::Pixels;

    pub const CONTENT_TYPE: &str = "image/png";
    pub const UNAVAILABLE: &str = "window snapshots are unavailable on this platform";

    pub fn pixels(_: &[u8]) -> Result<Pixels, String> {
        Err(UNAVAILABLE.to_owned())
    }
}

/// A PNG's pixels as 8-bit RGBA.
#[cfg(any(windows, mobile, test))]
fn decode_png(image: &[u8]) -> Result<Pixels, String> {
    let mut decoder = png::Decoder::new(Cursor::new(image));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader
        .output_buffer_size()
        .ok_or("the snapshot is too large")?;
    let mut buffer = vec![0; size];
    let frame = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    buffer.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        other => return Err(format!("a snapshot of {other:?} pixels")),
    };
    Ok(Pixels {
        width: frame.width,
        height: frame.height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_a_region_at_the_display_scale() {
        // A 4x2 image at 2x: two CSS pixels wide.
        let rgba: Vec<u8> = (0..32).collect();
        let pixels = Pixels {
            width: 4,
            height: 2,
            rgba,
        };
        let region = Region {
            x: 1.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let crop = pixels.crop(region, 2.0);
        assert_eq!((crop.width, crop.height), (2, 2));
        assert_eq!(&crop.rgba[..8], &[8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(&crop.rgba[8..], &[24, 25, 26, 27, 28, 29, 30, 31]);
        let png = encode(&crop).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }

    /// Windows and phone snapshots are PNGs, whose pixels a report crops.
    #[test]
    fn reads_back_a_png_snapshot() {
        let rgba: Vec<u8> = (0..32).collect();
        let png = encode(&Pixels {
            width: 4,
            height: 2,
            rgba: rgba.clone(),
        })
        .unwrap();
        let pixels = decode_png(&png).unwrap();
        assert_eq!((pixels.width, pixels.height), (4, 2));
        assert_eq!(pixels.rgba, rgba);
    }
}
