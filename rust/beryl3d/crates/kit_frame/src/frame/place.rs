//! Location labels carried by scene elements.

/// Where an element is. A place's owner supplies its printable label and can distinguish
/// places that print together by giving them different variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The unlabelled flat layer.
    Layer,
    Named {
        label: &'static str,
        variant: u8,
    },
}

impl Place {
    #[must_use]
    pub const fn named(label: &'static str) -> Self {
        Self::with_variant(label, 0)
    }

    #[must_use]
    pub const fn with_variant(label: &'static str, variant: u8) -> Self {
        Self::Named { label, variant }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Layer => "",
            Self::Named { label, .. } => label,
        }
    }

    #[must_use]
    pub const fn variant(self) -> u8 {
        match self {
            Self::Layer => 0,
            Self::Named { variant, .. } => variant,
        }
    }
}
