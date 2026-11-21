//! The machine and provider labels that constrain where a ticket can run.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pins {
    pub machine: Option<String>,
    pub provider: Option<String>,
}

/// Reads the first nonempty pin of each kind, in the ticket's label order.
pub fn from_labels(labels: &[String]) -> Pins {
    let pin = |prefix| {
        labels
            .iter()
            .filter_map(|label| label.strip_prefix(prefix))
            .find(|name| !name.is_empty())
            .map(str::to_owned)
    };
    Pins {
        machine: pin("machine:"),
        provider: pin("provider:"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_and_provider_pins_are_independent() {
        let labels = [
            "needs:person",
            "provider:",
            "provider:codex",
            "machine:host",
            "machine:other",
        ]
        .map(str::to_owned);
        assert_eq!(
            from_labels(&labels),
            Pins {
                machine: Some("host".to_owned()),
                provider: Some("codex".to_owned())
            }
        );
        assert_eq!(from_labels(&["tier:opus".to_owned()]), Pins::default());
    }
}
