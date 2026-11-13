use tauri::{AppHandle, Manager};

pub(super) use super::decode_png as pixels;

pub const CONTENT_TYPE: &str = "image/png";

pub async fn snapshot(app: &AppHandle) -> Result<Vec<u8>, String> {
    let path = app
        .state::<beryl_mobile::Mobile<tauri::Wry>>()
        .snapshot()
        .await
        .map_err(|e| e.to_string())?;
    let image = std::fs::read(&path);
    let removed = std::fs::remove_file(&path);
    let image = image.map_err(|e| e.to_string())?;
    removed.map_err(|e| e.to_string())?;
    Ok(image)
}
