//! Whether the configured hub URL reaches the hub's read-only listener.
//!
//! The hub can serve a second, read-only listener for a public address
//! (`IGNITE_HUB_PUBLIC_LISTEN`). It answers every request except `GET` and `HEAD` with 405 and
//! `{"error":"this listener is read-only"}`. A machine whose `[hub] url` reaches it can read
//! the feed, so the plain reachability check passes, but every write the daemon makes (heartbeats,
//! records, metrics, landing leases) is refused. `doctor` finds this with one tokenless `POST`
//! that changes nothing: the writable hub answers it with 401, and the read-only listener
//! refuses it before it looks at credentials.

use std::time::Duration;

use ignite_core::config::HubConfig;

use super::Finding;

/// How long the probe waits. `doctor` stays cheap, and the hub has already answered a read.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// The message the read-only listener answers every write with.
const READ_ONLY_MESSAGE: &str = "this listener is read-only";

/// A tokenless `POST` to the hub, to see which listener answers.
pub trait WriteProbe {
    /// The status and body the hub at `url` answers a tokenless `POST` to `path` with, or
    /// `None` when it does not answer in time.
    fn post_without_token(&self, url: &str, path: &str) -> Option<(u16, String)>;
}

/// The probe over HTTP.
pub struct HttpProbe;

impl WriteProbe for HttpProbe {
    fn post_without_token(&self, url: &str, path: &str) -> Option<(u16, String)> {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(PROBE_TIMEOUT))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let mut response = agent
            .post(format!("{}{path}", url.trim_end_matches('/')))
            .header("Content-Type", "application/json")
            .send("{}")
            .ok()?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string().unwrap_or_default();
        Some((status, body))
    }
}

/// A problem when the hub URL reaches the read-only listener; `None` when it does not, or when
/// the probe gets no answer (the `hub` finding already reports an unreachable hub).
pub fn finding(hub: &HubConfig, probe: &dyn WriteProbe) -> Option<Finding> {
    let path = format!("/{}/api/machines/heartbeat", hub.project);
    let (status, body) = probe.post_without_token(&hub.url, &path)?;
    (status == 405 && body.contains(READ_ONLY_MESSAGE)).then(|| {
        // Not labelled `hub`: `report` exits with "unreachable" for a failed `hub` finding, and
        // this hub answers.
        Finding::problem(
            "listener",
            format!(
                "the hub URL {} reaches the hub's read-only listener, which refuses every \
                 write this machine makes.",
                hub.url
            ),
            "point the hub URL at the writable listener; the read-only listener is for the \
             public address",
        )
    })
}
