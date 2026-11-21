//! The face descriptions the renderer and asset workers read.

use crate::atlas::FaceId;

/// The card back's [`FaceId`]: always the first face.
pub const CARD_BACK: FaceId = FaceId(0);

/// One distinct picture of the match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedFace {
    /// The picture's full-size scan on Scryfall, which also identifies it: two names that show
    /// the same scan share one face.
    pub image: String,
    /// The card and face names that show this picture; the first is drawn on a placeholder.
    /// Empty for the card back.
    pub names: Vec<String>,
    /// The face's colors as the card store gives them (`W`, `U`, `B`, `R`, `G`).
    pub colors: Vec<String>,
    /// The power and toughness printed on the face, when both are numbers: a battlefield
    /// plaque inks a value raised or lowered against them.
    pub printed: Option<[i32; 2]>,
    /// What Beryl's drawn frame prints on the face when its scan has not been downloaded.
    pub text: PrintedText,
    /// For a face that shows one ability on the stack, which ability: its source's picture
    /// wearing a plate that says the ability. `None` for a card's own face.
    pub plate: Option<Plate>,
}

/// The words a face prints, as the card store gives them, which Beryl's drawn frame sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrintedText {
    /// The mana cost in `{...}` notation, such as `{1}{U}{U}`.
    pub mana_cost: Option<String>,
    pub type_line: String,
    /// The rules text, one ability to a line, with symbols in `{...}` notation.
    pub rules: Option<String>,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    /// The face is a token's, which the frame draws in pewter where a card's is black.
    pub token: bool,
}

/// One ability of a face, said on a plate over the face's lower half, as an ability on the
/// stack shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plate {
    /// The face whose picture the plate covers.
    pub source: FaceId,
    /// A triggered ability rather than an activated one, which the plate's label says.
    pub triggered: bool,
    /// The paragraph of the source's rules text that says the ability.
    pub rules: String,
}
