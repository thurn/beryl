//! The dashboard's static files: the stylesheet and script, fingerprinted by content so a
//! browser can keep them for a year, the service worker at a fixed URL, and the icons.
//!
//! The files live in `static/` and are compiled in. [`serve`] answers a path relative to the
//! hub's root, so the server only has to try it before the pages.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::sync::OnceLock;

use sha2::{Digest, Sha256};

const STYLES: [&str; 5] = [
    include_str!("../../static/css/1-tokens.css"),
    include_str!("../../static/css/2-layout.css"),
    include_str!("../../static/css/3-components.css"),
    include_str!("../../static/css/4-motion.css"),
    include_str!("../../static/css/5-edit.css"),
];
/// The scripts, served as one file: cirrus itself, then ticket editing.
const SCRIPTS: [&str; 2] = [
    include_str!("../../static/cirrus.js"),
    include_str!("../../static/edit.js"),
];
const WORKER: &str = include_str!("../../static/sw.js");
const FAVICON_SVG: &[u8] = include_bytes!("../../static/favicon.svg");
const FAVICON_PNG: &[u8] = include_bytes!("../../static/favicon-32.png");
const APPLE_ICON: &[u8] = include_bytes!("../../static/apple-touch-icon.png");

const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Site-relative paths of the two fingerprinted assets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assets {
    pub css: String,
    pub js: String,
}

impl Assets {
    /// The paths of the files compiled into this build.
    pub fn current() -> Assets {
        let bundle = bundle();
        Assets {
            css: bundle.css_path.clone(),
            js: bundle.js_path.clone(),
        }
    }
}

struct Bundle {
    css: String,
    css_path: String,
    js: String,
    js_path: String,
}

fn fingerprint(content: &str, extension: &str) -> String {
    let digest = Sha256::digest(content.as_bytes());
    let prefix: String = digest
        .iter()
        .take(5)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("assets/cirrus.{prefix}.{extension}")
}

fn bundle() -> &'static Bundle {
    static BUNDLE: OnceLock<Bundle> = OnceLock::new();
    BUNDLE.get_or_init(|| {
        let css = STYLES.join("\n");
        let js = SCRIPTS.join("\n");
        Bundle {
            css_path: fingerprint(&css, "css"),
            js_path: fingerprint(&js, "js"),
            css,
            js,
        }
    })
}

/// One static file's response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub content_type: &'static str,
    pub cache_control: &'static str,
    pub body: Vec<u8>,
}

/// The file at `path`, relative to the hub's root and without a leading slash, if it is one of
/// the dashboard's.
pub fn serve(path: &str) -> Option<Asset> {
    let bundle = bundle();
    let (content_type, cache_control, body): (&'static str, &'static str, Vec<u8>) = if path
        == bundle.css_path
    {
        (
            "text/css; charset=utf-8",
            IMMUTABLE,
            bundle.css.clone().into_bytes(),
        )
    } else if path == bundle.js_path {
        (
            "text/javascript; charset=utf-8",
            IMMUTABLE,
            bundle.js.clone().into_bytes(),
        )
    } else {
        match path {
            "sw.js" => (
                "text/javascript; charset=utf-8",
                "no-cache",
                WORKER.as_bytes().to_vec(),
            ),
            "favicon.svg" => (
                "image/svg+xml",
                "public, max-age=86400",
                FAVICON_SVG.to_vec(),
            ),
            "favicon-32.png" => ("image/png", "public, max-age=86400", FAVICON_PNG.to_vec()),
            "apple-touch-icon.png" => ("image/png", "public, max-age=86400", APPLE_ICON.to_vec()),
            _ => return None,
        }
    };
    Some(Asset {
        content_type,
        cache_control,
        body,
    })
}
