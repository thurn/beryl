//! The window as it is drawn now, for a bug report's frozen frame: `WKWebView`'s own snapshot
//! of the page (`takeSnapshotWithConfiguration:completionHandler:`), which, unlike a screen
//! capture, needs no screen-recording permission and holds only the app's pixels, at the
//! display's scale.

use std::sync::mpsc::channel;
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// How long a snapshot may take before the report goes on without one.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The main window's page as a PNG.
pub async fn capture(app: &AppHandle) -> Result<Vec<u8>, String> {
    let window = app.get_webview_window("main").ok_or("no main window")?;
    let (send, receive) = channel::<Result<Vec<u8>, String>>();
    window
        .with_webview(move |webview| platform::snapshot(webview.inner(), send))
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

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Objective-C calls into WebKit and AppKit.
mod platform {
    use std::ffi::c_void;
    use std::sync::Mutex;
    use std::sync::mpsc::Sender;

    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;

    /// Asks `webview` (a `WKWebView`) for a snapshot and sends it as a PNG, once, to `send`.
    /// Runs on the main thread.
    pub fn snapshot(webview: *mut c_void, send: Sender<Result<Vec<u8>, String>>) {
        // The completion block is `Fn`, so the sender is taken out of a lock to send once.
        let send = Mutex::new(Some(send));
        let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            // SAFETY: WebKit passes a valid image or null, and a valid error or null, for the
            // block's duration.
            let result = match unsafe { (image.as_ref(), error.as_ref()) } {
                (Some(image), _) => png(image),
                (None, Some(error)) => Err(error.localizedDescription().to_string()),
                (None, None) => Err("WebKit returned no snapshot".to_owned()),
            };
            if let Some(send) = send.lock().ok().and_then(|mut s| s.take()) {
                let _ = send.send(result);
            }
        });
        // SAFETY: Tauri hands `with_webview` closures the window's live WKWebView, on the main
        // thread; a null configuration snapshots the whole visible page.
        unsafe {
            let webview = &*webview.cast::<WKWebView>();
            webview.takeSnapshotWithConfiguration_completionHandler(None, &done);
        }
    }

    fn png(image: &NSImage) -> Result<Vec<u8>, String> {
        let tiff = image
            .TIFFRepresentation()
            .ok_or("the snapshot has no bitmap")?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("the snapshot's bitmap")?;
        // SAFETY: an empty properties dictionary is valid for any file type.
        let data = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }
        .ok_or("the snapshot as a PNG")?;
        Ok(data.to_vec())
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::ffi::c_void;
    use std::sync::mpsc::Sender;

    pub fn snapshot(_: *mut c_void, send: Sender<Result<Vec<u8>, String>>) {
        let _ = send.send(Err("window snapshots are macOS only".to_owned()));
    }
}
