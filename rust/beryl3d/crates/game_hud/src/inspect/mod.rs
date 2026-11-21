//! What the game screen shows to look at cards closely: the inspector for one card, the viewer
//! of a pile's cards and the cards an effect revealed.

pub mod inspector;
pub mod revealed;
pub mod viewer;

/// What the viewer shows the cards of (`inspect::viewer`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewed {
    /// `side`'s library, graveyard or exile.
    Pile(
        beryl_protocol::protocol::Side,
        beryl_protocol::protocol::ZoneName,
    ),
    /// The faces of the Revealed Cards pile.
    Revealed,
}
