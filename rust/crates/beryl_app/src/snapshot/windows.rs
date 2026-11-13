use std::sync::mpsc::Sender;

use webview2_com::CapturePreviewCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, ICoreWebView2Controller,
};
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};

pub(super) use super::decode_png as pixels;

pub const CONTENT_TYPE: &str = "image/png";

/// Asks the webview `controller` holds for a snapshot and sends it, as a PNG, once, to
/// `send`. Runs on the main thread.
pub fn snapshot(webview: &tauri::webview::PlatformWebview, send: Sender<Result<Vec<u8>, String>>) {
    let failed = send.clone();
    if let Err(e) = start(&webview.controller(), send) {
        let _ = failed.send(Err(e.message()));
    }
}

fn start(
    controller: &ICoreWebView2Controller,
    send: Sender<Result<Vec<u8>, String>>,
) -> windows::core::Result<()> {
    // SAFETY: Tauri hands `with_webview` closures the window's live controller, on the main
    // thread, where WebView2 takes its calls; the handler holds the stream it reads.
    unsafe {
        let webview = controller.CoreWebView2()?;
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        let target = stream.clone();
        let done = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            let png = result.and_then(|()| read_all(&target));
            let _ = send.send(png.map_err(|e| e.message()));
            Ok(())
        }));
        webview.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &done,
        )
    }
}

fn read_all(stream: &IStream) -> windows::core::Result<Vec<u8>> {
    let mut png = Vec::new();
    let mut chunk = vec![0_u8; 1 << 16];
    // SAFETY: each read writes at most the chunk's length into it.
    unsafe {
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        loop {
            let mut read = 0;
            stream
                .Read(
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    Some(&raw mut read),
                )
                .ok()?;
            if read == 0 {
                return Ok(png);
            }
            png.extend_from_slice(&chunk[..read as usize]);
        }
    }
}
