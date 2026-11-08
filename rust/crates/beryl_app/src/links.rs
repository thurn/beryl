//! Links out of the app, such as the credits' pages for the music and sound packs, open in the
//! system's browser: the window navigates only among the app's own pages.

use std::process::Command;

use beryl_session::journal;
use serde_json::json;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Runtime, Url};

/// The plugin that turns the window's navigations to the web into the browser's.
pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("links")
        .on_navigation(|_, url| {
            if !external(url) {
                return true;
            }
            if let Err(e) = open(url) {
                journal::warn("app", format!("could not open {url}: {e}"), json!(null));
            }
            false
        })
        .build()
}

/// Whether `url` is a page on the web rather than the app's own: those are served from
/// `tauri://localhost`, `http://tauri.localhost` on Windows, or Vite on localhost in `just dev`.
fn external(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && !matches!(
            url.host_str(),
            Some("localhost" | "tauri.localhost" | "127.0.0.1")
        )
}

fn open(url: &Url) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        Command::new("xdg-open")
    };
    command.arg(url.as_str()).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_pages_leave_the_app() {
        let external = |s: &str| external(&Url::parse(s).unwrap());
        assert!(external("https://kenney.nl/assets/casino-audio"));
        assert!(external("http://example.com/"));
        assert!(!external("tauri://localhost/#/game"));
        assert!(!external("http://tauri.localhost/"));
        assert!(!external("http://localhost:1420/#/"));
        assert!(!external("beryl-art://localhost/backs/x.jpg"));
    }
}
