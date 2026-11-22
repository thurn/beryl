//! The reproduction command for a fault in a game recording.

use std::fmt::Write;

use beryl_protocol::protocol::Side;
use kit_base::data::profiles::SizeClass;
use kit_probe::agent::{Context, Fault};

#[must_use]
pub fn print_fault(fault: &Fault, recording: &str, seat: Side, context: &Context) -> String {
    let seat = match seat {
        Side::You => "you",
        Side::Opponent => "opponent",
    };
    let size = format!("{}x{}", context.size.width, context.size.height);
    let mut command = format!("just b3d tree {recording} --seat {seat}");
    if context.layout.size_class() == SizeClass::Compact {
        command.push_str(" --layout phone");
    }
    let _ = write!(command, " --size {size}");
    fault.print_repro(&format!("b3d:{recording}/{seat}/{size}"), &command)
}
