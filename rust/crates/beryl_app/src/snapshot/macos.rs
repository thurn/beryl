use std::sync::Mutex;
use std::sync::mpsc::Sender;

use block2::RcBlock;
use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep, NSImage};
use objc2_foundation::{MainThreadMarker, NSData, NSError};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

use super::Pixels;

pub const CONTENT_TYPE: &str = "image/tiff";

/// Asks `webview` (a `WKWebView`) for a snapshot and sends it, as an uncompressed TIFF,
/// once, to `send`. Runs on the main thread.
pub fn snapshot(webview: &tauri::webview::PlatformWebview, send: Sender<Result<Vec<u8>, String>>) {
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
        let webview = &*webview.inner().cast::<WKWebView>();
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
