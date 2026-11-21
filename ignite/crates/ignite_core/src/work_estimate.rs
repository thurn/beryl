//! The estimated agent effort left in a ticket or an epic.

use std::fmt::Write as _;

use crate::protocol::tickets::{Status, Ticket, TicketType};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkRemaining {
    /// Estimated effort on closed tickets.
    pub closed_minutes: u64,
    /// Closed tickets without an estimate, which leave the total scope uncertain.
    pub unestimated_closed: u64,
    /// Estimates on tickets that are still open, blocked, deferred or in progress.
    pub filed_minutes: u64,
    /// Work expected to become tickets later, including reserves on closed tickets.
    pub unfiled_minutes: u64,
    /// Open work that has no estimate, including tickets awaiting approval.
    pub unestimated: u64,
}

impl WorkRemaining {
    pub fn include(&mut self, ticket: &Ticket) {
        if ticket.ticket_type != TicketType::Meta {
            self.unfiled_minutes += u64::from(ticket.unfiled_minutes.unwrap_or(0));
        }
        if !has_own_work(ticket) {
            return;
        }
        match (ticket.status, ticket.estimate_minutes) {
            (Status::Closed, Some(minutes)) => self.closed_minutes += u64::from(minutes),
            (Status::Closed, None) => self.unestimated_closed += 1,
            (_, Some(minutes)) => self.filed_minutes += u64::from(minutes),
            (_, None) => self.unestimated += 1,
        }
    }

    /// A display percentage based on the whole known scope. Round down so every nonzero
    /// estimate for work left keeps the percentage below 100.
    pub fn percent_complete(self) -> Option<u64> {
        let remaining = self.filed_minutes + self.unfiled_minutes;
        if remaining == 0 && self.unestimated == 0 {
            return Some(100);
        }
        if self.unestimated > 0 || self.unestimated_closed > 0 {
            return None;
        }
        Some(100 * self.closed_minutes / (self.closed_minutes + remaining))
    }

    pub fn breakdown(self) -> String {
        let mut text = format!(
            "{} estimated closed · {}",
            duration(self.closed_minutes),
            self.summary()
        );
        if self.unestimated_closed > 0 {
            let noun = if self.unestimated_closed == 1 {
                "ticket"
            } else {
                "tickets"
            };
            let _ = write!(
                text,
                " · {} unestimated closed {noun}",
                self.unestimated_closed
            );
        }
        text.push_str(" (agent time)");
        text
    }

    pub fn summary(self) -> String {
        let minutes = self.filed_minutes + self.unfiled_minutes;
        let mut parts = Vec::new();
        if minutes > 0 {
            parts.push(format!("{} estimated left", duration(minutes)));
        }
        if self.unfiled_minutes > 0 {
            parts.push(format!("{} unfiled", duration(self.unfiled_minutes)));
        }
        if self.unestimated > 0 {
            let noun = if self.unestimated == 1 {
                "ticket"
            } else {
                "tickets"
            };
            parts.push(format!("{} unestimated {noun}", self.unestimated));
        }
        if parts.is_empty() {
            return "No work remaining".to_owned();
        }
        parts.join(" · ")
    }
}

pub fn needs_estimate(ticket: &Ticket) -> bool {
    ticket.status != Status::Closed && has_own_work(ticket) && ticket.estimate_minutes.is_none()
}

fn has_own_work(ticket: &Ticket) -> bool {
    !matches!(
        ticket.ticket_type,
        TicketType::Epic | TicketType::Milestone | TicketType::Meta
    )
}

pub fn duration(minutes: u64) -> String {
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}
