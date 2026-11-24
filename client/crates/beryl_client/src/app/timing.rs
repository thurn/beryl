//! Connects the desktop host's trace to the frame log and bug reports.

use std::time::Instant;

use app_client::Client;
use app_client::command::Request;
use beryl_host::Ticked;
use beryl_host::timing::Timing;
use beryl_session::script::Verb;

use super::{AppFeed, Source};
use crate::frame_log::FrameLog;

pub(super) fn start(feed: &AppFeed, epoch: Instant) {
    if let Some(Ok(log)) = feed.log.borrow_mut().as_mut() {
        log.start_trace(epoch);
    }
}

pub(super) fn record(feed: &AppFeed, event: &Timing) {
    if let Some(Ok(log)) = feed.log.borrow_mut().as_mut() {
        if let Timing::Frame(frame) = event {
            let frame = beryl_host::timing::FrameTiming {
                searching: feed.searching,
                ..*frame
            };
            log.timing(&Timing::Frame(frame));
        } else {
            log.timing(event);
        }
    }
}

pub(super) fn freeze(feed: &AppFeed) {
    if let Some(Ok(log)) = feed.log.borrow_mut().as_mut() {
        log.freeze_trace();
    }
}

pub(super) fn save(feed: &mut AppFeed, request: &Request) {
    let log = feed
        .log
        .borrow_mut()
        .as_mut()
        .and_then(|result| result.as_mut().ok())
        .map(FrameLog::bug_attachment);
    let live = match &feed.source {
        Some(Source::Live(commands)) => Some(commands),
        _ => None,
    };
    feed.bugs.save(request, log, live);
}

pub(super) fn legacy_frame(log: &mut FrameLog, client: &Client, ticked: &Ticked, searching: bool) {
    let verbs: Vec<&str> = if log.keeps_late_verbs() && FrameLog::is_late(ticked) {
        client.playing_verbs().map(Verb::name).collect()
    } else {
        Vec::new()
    };
    log.frame(ticked, &verbs, searching);
}
