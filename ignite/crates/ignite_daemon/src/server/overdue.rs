//! The server's part of ending verb processes that ignore a stop (see [`crate::stop_overdue`]).

use super::Server;

impl Server {
    /// Ends the verb processes whose stop was asked [`crate::stop_overdue::WAIT`] ago and that
    /// still run, and logs each. Does nothing for a server without a local database or verb
    /// processes.
    pub fn end_overdue_stops(&self) {
        let (Some(db), Some(processes)) = (&self.db, &self.processes) else {
            return;
        };
        let Some(ender) = &processes.ender else {
            return;
        };
        for line in crate::stop_overdue::end_overdue(
            db,
            self.ctx.clock.as_ref(),
            ender.as_ref(),
            processes.stop_wait,
        ) {
            eprintln!("ignite: {line}");
        }
    }
}
