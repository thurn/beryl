//! Card images, served to the UI over the `beryl-art` URI scheme from `beryl_art`'s cache.
//!
//! The UI asks for the same path as a card's image URL on Scryfall's image host under
//! `beryl-art://localhost/`, a card back's under `beryl-art://localhost/backs/`. A failure
//! answers with an error status, which the UI shows as a placeholder.

use beryl_art::{ArtCache, ArtError};
use tauri::http::{Response, StatusCode, header};

pub const SCHEME: &str = "beryl-art";

/// The `beryl-art` response for a request path.
pub fn respond(cache: &ArtCache, path: &str) -> Response<Vec<u8>> {
    let (status, body) = match cache.get(path) {
        Ok(bytes) => (StatusCode::OK, bytes),
        Err(e) => {
            beryl_session::journal::warn("art", format!("{path}: {e}"), serde_json::Value::Null);
            let status = match e {
                ArtError::NotAnImage => StatusCode::NOT_FOUND,
                ArtError::Download(_) | ArtError::Io(_) => StatusCode::BAD_GATEWAY,
            };
            (status, Vec::new())
        }
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "image/jpeg")
        .body(body)
        .expect("a static response is valid")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use beryl_art::Origins;

    use super::*;

    const ID: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

    #[test]
    fn a_hit_answers_ok_and_a_non_image_path_answers_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = format!("/art_crop/front/0/a/{ID}.jpg");
        let file = dir.path().join(&path[1..]);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"cached").unwrap();
        let unused = Origins {
            cards: "http://127.0.0.1:1",
            backs: "http://127.0.0.1:1",
        };
        let cache = ArtCache::new(dir.path().to_owned(), unused);

        let hit = respond(&cache, &path);
        assert_eq!(hit.status(), StatusCode::OK);
        assert_eq!(hit.body(), b"cached");
        assert_eq!(respond(&cache, "/").status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn a_failed_download_answers_a_bad_gateway() {
        let dir = tempfile::tempdir().unwrap();
        let unreachable = Origins {
            cards: "http://127.0.0.1:1",
            backs: "http://127.0.0.1:1",
        };
        let cache = ArtCache::new(dir.path().to_owned(), unreachable);
        let path = format!("/art_crop/front/0/a/{ID}.jpg");

        assert_eq!(respond(&cache, &path).status(), StatusCode::BAD_GATEWAY);
    }
}
