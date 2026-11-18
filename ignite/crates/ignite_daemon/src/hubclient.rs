//! The daemon's HTTP client for the hub, on `ureq`.
//!
//! Every request carries the machine's token as `Authorization: Bearer <token>` and the writer as
//! `X-Actor`. A request that cannot reach the hub, or that the hub answers with a server error,
//! is tried again a few times; after that it fails with [`Error::Unreachable`], which verbs
//! report with exit code 4.

use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use ignite_core::config::HubConfig;
use ignite_core::ids::Seq;
use ignite_core::protocol::hub::{
    CachedResult, Claim, FlakyReport, Heartbeat, HeartbeatReply, Landed, LookupReply,
    LookupRequest, MachineFree, MachineRegister, MachineRegistered, MetricsImport, NewResult,
    Record, RecordBatch, Reservation, ResultKey, StoreRequest, TicketBrief,
};
use ignite_core::protocol::tickets::{FeedBatch, Lease, LeaseBusy, LeaseOp, LeaseReply};
use ignite_core::traits::{Error, Hub, Result};

/// How long an ordinary request may take before it counts as unreachable.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The hub at `base` (`<url>/<project>/api`), writing as `actor` with the machine's token.
#[derive(Clone, Debug)]
pub struct HubClient {
    pub base: String,
    pub token: String,
    pub actor: String,
    /// How many times a failed request is tried again.
    pub retries: u32,
    /// The pause before the first retry; each later retry waits one pause longer.
    pub retry_pause: Duration,
    agent: ureq::Agent,
}

/// A response the hub sent, with its status.
struct Answer {
    status: u16,
    body: Vec<u8>,
}

impl HubClient {
    /// The client for the hub that `ignite.toml` names.
    pub fn new(hub: &HubConfig, token: &str, actor: &str) -> HubClient {
        let base = format!("{}/{}/api", hub.url.trim_end_matches('/'), hub.project);
        HubClient::with_base(base, token, actor)
    }

    /// The client for the API at `base`.
    pub fn with_base(base: impl Into<String>, token: &str, actor: &str) -> HubClient {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build();
        HubClient {
            base: base.into(),
            token: token.to_owned(),
            actor: actor.to_owned(),
            retries: 3,
            retry_pause: Duration::from_millis(500),
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// `POST /machines/register`.
    pub fn register(&self, machine: &MachineRegister) -> Result<MachineRegistered> {
        self.post("/machines/register", machine)
    }

    /// `POST /machines/heartbeat`.
    pub fn heartbeat(&self, heartbeat: &Heartbeat) -> Result<HeartbeatReply> {
        self.post("/machines/heartbeat", heartbeat)
    }

    fn post<B: Serialize, R: DeserializeOwned>(&self, path: &str, body: &B) -> Result<R> {
        let answer = self.post_raw(path, body)?;
        decode(path, &ok(path, answer)?.body)
    }

    fn post_raw<B: Serialize>(&self, path: &str, body: &B) -> Result<Answer> {
        let bytes = serde_json::to_vec(body).map_err(|e| Error::Invalid(e.to_string()))?;
        let url = format!("{}{path}", self.base);
        self.with_retries(path, || {
            self.authorized(self.agent.post(&url))
                .header("Content-Type", "application/json")
                .send(&bytes[..])
        })
    }

    fn authorized<B>(&self, request: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        request
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("X-Actor", &self.actor)
    }

    /// Sends a request, trying again while the hub cannot be reached or answers with a server
    /// error.
    fn with_retries(
        &self,
        path: &str,
        send: impl Fn() -> std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<Answer> {
        let mut last = String::new();
        for attempt in 0..=self.retries {
            if attempt > 0 {
                std::thread::sleep(self.retry_pause * attempt);
            }
            match send() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response
                        .into_body()
                        .read_to_vec()
                        .map_err(|e| Error::Unreachable(format!("{path}: {e}")));
                    match body {
                        Ok(body) if status < 500 => return Ok(Answer { status, body }),
                        Ok(body) => last = format!("{status} {}", String::from_utf8_lossy(&body)),
                        Err(Error::Unreachable(message)) => last = message,
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => last = error.to_string(),
            }
        }
        Err(Error::Unreachable(format!(
            "the hub at {}{path}: {last}",
            self.base
        )))
    }
}

/// The answer if its status is a success, else the error its status means.
fn ok(path: &str, answer: Answer) -> Result<Answer> {
    let message = || format!("{path}: {}", String::from_utf8_lossy(&answer.body));
    match answer.status {
        200..=299 => Ok(answer),
        404 => Err(Error::NotFound(message())),
        409 => Err(Error::Conflict(message())),
        401 | 403 => Err(Error::Invalid(format!(
            "the hub refused this machine's token: {}",
            message()
        ))),
        _ => Err(Error::Invalid(format!("{} {}", answer.status, message()))),
    }
}

fn decode<R: DeserializeOwned>(path: &str, body: &[u8]) -> Result<R> {
    serde_json::from_slice(body).map_err(|e| Error::Invalid(format!("{path}: bad reply: {e}")))
}

impl Hub for HubClient {
    fn lookup(&self, keys: &[ResultKey]) -> Result<Vec<Option<CachedResult>>> {
        let request = LookupRequest {
            keys: keys.to_vec(),
        };
        let reply: LookupReply = self.post("/results/lookup", &request)?;
        Ok(reply.results)
    }

    fn store(&self, results: &[NewResult]) -> Result<()> {
        let request = StoreRequest {
            results: results.to_vec(),
            areas: Vec::new(),
        };
        let _: serde_json::Value = self.post("/results/store", &request)?;
        Ok(())
    }

    /// The hub answers `POST /next` with a reservation, or `null` when it has nothing.
    fn next(&self, free: &MachineFree) -> Result<Option<Reservation>> {
        self.post("/next", free)
    }

    fn claim(&self, claim: &Claim) -> Result<TicketBrief> {
        self.post("/claims", claim)
    }

    fn landed(&self, landed: &Landed) -> Result<()> {
        let _: serde_json::Value = self.post("/landed", landed)?;
        Ok(())
    }

    fn lease(&self, name: &str, op: LeaseOp) -> Result<LeaseReply> {
        let path = format!("/leases/{name}");
        let answer = self.post_raw(&path, &op)?;
        if answer.status == 409 {
            return Ok(LeaseReply::Busy(decode::<LeaseBusy>(&path, &answer.body)?));
        }
        Ok(LeaseReply::Held(decode::<Lease>(
            &path,
            &ok(&path, answer)?.body,
        )?))
    }

    fn feed(&self, since: Seq, wait_s: u32) -> Result<FeedBatch> {
        let url = format!("{}/feed", self.base);
        let timeout = REQUEST_TIMEOUT + Duration::from_secs(u64::from(wait_s));
        let answer = self.with_retries("/feed", || {
            self.authorized(self.agent.get(&url))
                .query("since", since.0.to_string())
                .query("wait", wait_s.to_string())
                .config()
                .timeout_global(Some(timeout))
                .build()
                .call()
        })?;
        decode("/feed", &ok("/feed", answer)?.body)
    }

    fn records(&self, batch: &[Record]) -> Result<()> {
        let request = RecordBatch {
            records: batch.to_vec(),
        };
        let _: serde_json::Value = self.post("/records", &request)?;
        Ok(())
    }

    fn metrics(&self, import: &MetricsImport) -> Result<()> {
        let _: serde_json::Value = self.post("/metrics/import", import)?;
        Ok(())
    }

    fn flaky(&self, report: &FlakyReport) -> Result<()> {
        let _: serde_json::Value = self.post("/flaky", report)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "hubclient_tests.rs"]
pub(crate) mod tests;
