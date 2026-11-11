//! The spec server's bug report routes' guard and download: who may write or fetch reports,
//! and the bundle of the active ones (`GET /bug-reports.zip`).

use std::collections::HashMap;
use std::io::{self, Seek, Write};
use std::net::TcpStream;

use beryl_session::bugs;

use crate::serve::respond;

/// Report writes can create local beads, and bundles contain private diagnostics.
/// Accept the loopback Vite proxy's same-origin requests and local command-line clients.
pub(super) fn local_request(headers: &HashMap<String, String>) -> bool {
    let Some(host) = headers.get("host") else {
        return false;
    };
    let (name, port) = host.split_once(':').unwrap_or((host, "80"));
    matches!(name, "localhost" | "127.0.0.1")
        && port.parse::<u16>().is_ok()
        && headers
            .get("origin")
            .is_none_or(|origin| origin.strip_prefix("http://") == Some(host))
        && headers
            .get("sec-fetch-site")
            .is_none_or(|site| matches!(site.as_str(), "same-origin" | "none"))
}

/// `GET /bug-reports.zip`: the active local reports, zipped.
pub(super) fn export(stream: &mut TcpStream) -> Result<(), String> {
    let bundle = (|| -> io::Result<_> {
        let mut bundle = tempfile::tempfile()?;
        let folders = bugs::archive::active(&bugs::dir())?;
        bugs::archive::write(&folders, &mut bundle)?;
        bundle.rewind()?;
        Ok((bundle.metadata()?.len(), bundle))
    })();
    let (length, mut bundle) = match bundle {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("specs: could not export bug reports: {error}");
            return respond(stream, "500 Internal Server Error", "{}");
        }
    };
    // Keep the ZIP off the spec API's cross-origin JSON response path.
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\n\
         Content-Disposition: attachment; filename=beryl-bug-reports.zip\r\n\
         Cache-Control: no-store\r\nCross-Origin-Resource-Policy: same-origin\r\n\
         Content-Length: {length}\r\nConnection: close\r\n\r\n"
    )
    .and_then(|()| io::copy(&mut bundle, stream).map(|_| ()))
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_routes_only_accept_local_requests() {
        let headers = |host: &str, origin: &str, site: &str| {
            [
                ("host".to_owned(), host.to_owned()),
                ("origin".to_owned(), origin.to_owned()),
                ("sec-fetch-site".to_owned(), site.to_owned()),
            ]
            .into_iter()
            .collect()
        };
        assert!(local_request(&headers(
            "localhost:1430",
            "http://localhost:1430",
            "same-origin"
        )));
        assert!(!local_request(&headers(
            "localhost:1430",
            "http://localhost:9999",
            "same-site"
        )));
        assert!(!local_request(&headers(
            "example.org:1430",
            "http://example.org:1430",
            "same-origin"
        )));
        let mut navigation = HashMap::from([("host".to_owned(), "127.0.0.1:1430".to_owned())]);
        assert!(local_request(&navigation));
        navigation.insert("sec-fetch-site".to_owned(), "cross-site".to_owned());
        assert!(!local_request(&navigation));
    }
}
