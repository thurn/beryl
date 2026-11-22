//! Wording for effects that insert phases into the current turn.

use crate::TurnPhase;

pub(super) fn additional_phases(after: TurnPhase, phases: &[TurnPhase]) -> Option<String> {
    if phases.is_empty() {
        return None;
    }
    let added = phases
        .iter()
        .map(|&phase| format!("an additional {} phase", name(phase)))
        .collect::<Vec<_>>()
        .join(" followed by ");
    Some(format!(
        "after this {} phase, there is {added}",
        name(after)
    ))
}

fn name(phase: TurnPhase) -> &'static str {
    match phase {
        TurnPhase::Beginning => "beginning",
        TurnPhase::Main => "main",
        TurnPhase::Combat => "combat",
        TurnPhase::Ending => "ending",
    }
}
