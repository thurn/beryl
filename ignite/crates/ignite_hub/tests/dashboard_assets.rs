//! The static files: fingerprinted names, cache headers and the icons.
#![allow(clippy::format_collect)]

use ignite_hub::dashboard::assets::{Assets, serve};
use ignite_hub::dashboard::common::SITE_ICONS;
use ignite_hub::dashboard::components::{PageOptions, document, page_regions};
use sha2::{Digest, Sha256};

fn prefix(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(5)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn the_stylesheet_and_script_are_served_under_an_immutable_content_hash() {
    let assets = Assets::current();
    for (path, extension) in [(&assets.css, "css"), (&assets.js, "js")] {
        let file = serve(path).unwrap();
        assert_eq!(file.cache_control, "public, max-age=31536000, immutable");
        assert_eq!(
            *path,
            format!("assets/cirrus.{}.{extension}", prefix(&file.body))
        );
    }
}

#[test]
fn the_service_worker_keeps_a_stable_url_and_revalidates() {
    let file = serve("sw.js").unwrap();
    assert_eq!(file.cache_control, "no-cache");
    assert!(
        String::from_utf8(file.body)
            .unwrap()
            .contains("addEventListener(\"fetch\"")
    );
}

#[test]
fn project_pages_link_to_the_assets_and_keep_their_own_root() {
    let options = PageOptions {
        title: "Tickets".into(),
        root: "/proj/".into(),
        icons: Some(SITE_ICONS),
        brand: "Proj".into(),
        asset_root: Some("/".into()),
        ..PageOptions::default()
    };
    let html = document(&options, &page_regions(&options));
    let assets = Assets::current();
    for expected in [
        "<meta name=\"cirrus-root\" content=\"/proj/\">".to_owned(),
        format!("href=\"/{}\"", assets.css),
        format!("src=\"/{}\"", assets.js),
        "<link rel=\"icon\" type=\"image/svg+xml\" sizes=\"any\" href=\"/favicon.svg\">".to_owned(),
        "<link rel=\"apple-touch-icon\" sizes=\"180x180\" href=\"/apple-touch-icon.png\">"
            .to_owned(),
        "<meta name=\"apple-mobile-web-app-title\" content=\"Proj\">".to_owned(),
    ] {
        assert!(html.contains(&expected), "{expected}");
    }
}

#[test]
fn the_icons_are_square_pngs_of_the_required_size_and_a_scalable_svg() {
    for (path, size) in [(SITE_ICONS.png, 32_u32), (SITE_ICONS.apple, 180)] {
        let file = serve(path.trim_start_matches('/')).unwrap();
        assert_eq!(file.content_type, "image/png");
        assert_eq!(file.body[..8], [137, 80, 78, 71, 13, 10, 26, 10]);
        let dimension = |at: usize| u32::from_be_bytes(file.body[at..at + 4].try_into().unwrap());
        assert_eq!((dimension(16), dimension(20)), (size, size), "{path}");
    }
    let svg = serve("favicon.svg").unwrap();
    assert_eq!(svg.content_type, "image/svg+xml");
    assert!(
        String::from_utf8(svg.body)
            .unwrap()
            .contains("viewBox=\"0 0 256 256\"")
    );
    assert!(serve("nothing.txt").is_none());
}
