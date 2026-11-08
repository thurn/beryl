//! The window as it is drawn now, for a bug report's frozen frame: `WKWebView`'s own snapshot
//! of the page (`takeSnapshotWithConfiguration:completionHandler:`), which, unlike a screen
//! capture, needs no screen-recording permission and holds only the app's pixels, at the
//! display's scale.
//!
//! The pixels stay here. [`capture`] keeps the snapshot as `WebKit`'s uncompressed TIFF, a few
//! milliseconds' work, and hands the UI a number; the UI shows it from
//! `beryl-art://localhost/snapshot/<n>` ([`Snapshots::respond`]), which `WebKit` decodes at once.
//! Only a report that is written pays for PNGs, cropped and encoded here ([`Snapshots::pngs`]).
//! Encoding a retina window to PNG and sending it through IPC as base64, for the UI to decode
//! and encode again, held the reporter for seconds.

use std::io::Cursor;
use std::sync::mpsc::channel;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use beryl_session::bugs::{Images, Region};
use tauri::http::{Response, StatusCode};
use tauri::{AppHandle, Manager};

/// How long a snapshot may take before the report goes on without one.
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

    /// Serves snapshot `<n>` at `/snapshot/<n>`, as a TIFF.
    pub fn respond(&self, path: &str) -> Response<Vec<u8>> {
        let wanted = path.strip_prefix(PATH).and_then(|n| n.parse::<u32>().ok());
        let found = self
            .lock()
            .as_ref()
            .filter(|(id, _)| Some(*id) == wanted)
            .map(|(_, tiff)| tiff.clone());
        match found {
            Some(tiff) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "image/tiff")
                .header("Cache-Control", "no-store")
                .body(tiff),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new()),
        }
        .expect("a snapshot response is valid")
    }

    /// Snapshot `id` as PNGs: the whole window, and `region` of it (in CSS pixels of a page
    /// `page_width` wide), if given.
    pub fn pngs(&self, id: u32, region: Option<Region>, page_width: f64) -> Result<Images, String> {
        let tiff = self
            .lock()
            .as_ref()
            .filter(|(n, _)| *n == id)
            .map(|(_, tiff)| tiff.clone())
            .ok_or_else(|| format!("no snapshot {id}"))?;
        let pixels = platform::pixels(&tiff)?;
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
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let (send, receive) = channel::<Result<Vec<u8>, String>>();
    window
        .with_webview(move |webview| platform::snapshot(webview.inner(), send))
        .map_err(|e| e.to_string())?;
    // The snapshot completes on the main thread, which this must not block.
    let tiff = tauri::async_runtime::spawn_blocking(move || {
        receive
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|e| Err(format!("no snapshot: {e}")))
    })
    .await
    .map_err(|e| e.to_string())??;
    let snapshots = app.state::<Snapshots>();
    let mut latest = snapshots.lock();
    let id = latest.as_ref().map_or(1, |(n, _)| n + 1);
    *latest = Some((id, tiff));
    Ok(id)
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
mod platform {
    use std::ffi::c_void;
    use std::sync::Mutex;
    use std::sync::mpsc::Sender;

    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData, NSError};
    use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

    use super::Pixels;

    /// Asks `webview` (a `WKWebView`) for a snapshot and sends it, as an uncompressed TIFF,
    /// once, to `send`. Runs on the main thread.
    pub fn snapshot(webview: *mut c_void, send: Sender<Result<Vec<u8>, String>>) {
        // The completion block is `Fn`, so the sender is taken out of a lock to send once.
        let send = Mutex::new(Some(send));
        let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            // SAFETY: WebKit passes a valid image or null, and a valid error or null, for the
            // block's duration.
            let result = match unsafe { (image.as_ref(), error.as_ref()) } {
                (Some(image), _) => image
                    .TIFFRepresentation()
                    .map(|data| data.to_vec())
                    .ok_or_else(|| "the snapshot has no bitmap".to_owned()),
                (None, Some(error)) => Err(error.localizedDescription().to_string()),
                (None, None) => Err("WebKit returned no snapshot".to_owned()),
            };
            if let Some(send) = send.lock().ok().and_then(|mut s| s.take()) {
                let _ = send.send(result);
            }
        });
        // SAFETY: Tauri hands `with_webview` closures the window's live WKWebView, on the main
        // thread. The page is frozen already, so the snapshot need not wait for the next screen
        // update: it is what shows now.
        unsafe {
            let config = WKSnapshotConfiguration::new(MainThreadMarker::new_unchecked());
            config.setAfterScreenUpdates(false);
            let webview = &*webview.cast::<WKWebView>();
            webview.takeSnapshotWithConfiguration_completionHandler(Some(&config), &done);
        }
    }

    /// A TIFF's pixels as 8-bit RGBA: `AppKit`'s bitmap of it, read off whichever thread.
    pub fn pixels(tiff: &[u8]) -> Result<Pixels, String> {
        let bitmap = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(tiff))
            .ok_or("the snapshot is not a bitmap")?;
        let width = usize::try_from(bitmap.pixelsWide()).map_err(|e| e.to_string())?;
        let height = usize::try_from(bitmap.pixelsHigh()).map_err(|e| e.to_string())?;
        let samples = bitmap.samplesPerPixel();
        let format = bitmap.bitmapFormat();
        let row = usize::try_from(bitmap.bytesPerRow()).map_err(|e| e.to_string())?;
        if bitmap.isPlanar()
            || bitmap.bitsPerSample() != 8
            || !(samples == 3 || samples == 4)
            || format.contains(NSBitmapFormat::FloatingPointSamples)
        {
            return Err(format!(
                "a snapshot bitmap of {samples} samples of {} bits, format {format:?}",
                bitmap.bitsPerSample()
            ));
        }
        let samples = samples as usize;
        let stride = usize::try_from(bitmap.bitsPerPixel()).map_err(|e| e.to_string())? / 8;
        let alpha_first = format.contains(NSBitmapFormat::AlphaFirst);
        let data = bitmap.bitmapData();
        if data.is_null() {
            return Err("the snapshot bitmap has no data".to_owned());
        }
        // SAFETY: a meshed bitmap's data is `bytesPerRow * pixelsHigh` bytes, alive as long as
        // `bitmap`, which outlives this slice.
        let bytes = unsafe { std::slice::from_raw_parts(data, row * height) };
        let mut rgba = Vec::with_capacity(width * height * 4);
        // WebKit's snapshot is RGBA already: copied a row at a time, not a pixel.
        if samples == 4 && stride == 4 && !alpha_first {
            for y in 0..height {
                rgba.extend_from_slice(&bytes[y * row..][..width * 4]);
            }
            return Ok(Pixels {
                width: width as u32,
                height: height as u32,
                rgba,
            });
        }
        for y in 0..height {
            for x in 0..width {
                let p = &bytes[y * row + x * stride..][..samples];
                match (samples, alpha_first) {
                    (4, true) => rgba.extend_from_slice(&[p[1], p[2], p[3], p[0]]),
                    (4, false) => rgba.extend_from_slice(p),
                    _ => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
                }
            }
        }
        Ok(Pixels {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;

    use super::Pixels;

    pub fn snapshot(_: *mut c_void, send: Sender<Result<Vec<u8>, String>>) {
        let _ = send.send(Err("window snapshots are macOS only".to_owned()));
    }

    pub fn pixels(_: &[u8]) -> Result<Pixels, String> {
        Err("window snapshots are macOS only".to_owned())
    }
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
}
