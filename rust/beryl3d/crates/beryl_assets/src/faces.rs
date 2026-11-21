//! A match's faces as the asset workers prepare them (docs/beryl-3d.md, "Card faces",
//! "Preparation"). The client supplies the faces a match shows, in
//! [`FaceId`] order; each face's index in it is also its layer in the board-size texture array.
//! [`MatchFaces`] adds where each face's scan is found and how its compressed picture is cached.

use kit_base::atlas::FaceId;
use kit_base::faces::{ListedFace, Plate, PrintedText};

pub use kit_base::faces::CARD_BACK;

/// Scryfall's card image host, which `card_records::image` puts before a face's path.
const IMAGES: &str = "https://cards.scryfall.io";
/// Scryfall's card back host, which `card_records` puts before the card back's path.
const BACKS: &str = "https://backs.scryfall.io";

/// One face of the match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchFace {
    /// The scan's path for `beryl_art::ArtCache::request`, such as
    /// `/large/front/a/b/ab12….jpg`, or `/backs/large/0/a/0aee….jpg` for the card back.
    pub scan: String,
    /// The picture's identity in the compressed cache: the printing's Scryfall id and the side,
    /// or `back-` and the card back's id.
    pub key: String,
    /// The card and face names that show this picture; the first is the name Beryl's drawn
    /// frame prints.
    pub names: Vec<String>,
    /// The face's colors as the card store gives them (`W`, `U`, `B`, `R`, `G`), which choose
    /// the drawn frame's palette.
    pub colors: Vec<String>,
    /// The words the drawn frame prints.
    pub text: PrintedText,
    /// For a face that shows an ability on the stack, the ability its plate says.
    pub plate: Option<Plate>,
}

/// Every face of a match, in [`FaceId`] order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatchFaces {
    pub faces: Vec<MatchFace>,
}

impl MatchFaces {
    /// The faces of `list`, with the same [`FaceId`]s.
    #[must_use]
    pub fn new(list: &[ListedFace]) -> Self {
        let faces = list
            .iter()
            .map(|face| {
                let (scan, mut key) = scan_and_key(&face.image);
                if let Some(plate) = &face.plate {
                    // A plate face shares its source's scan but is a picture of its own.
                    let kind = if plate.triggered { 't' } else { 'a' };
                    key = format!("{key}-plate-{kind}-{:08x}", fnv(&plate.rules));
                }
                // A plate face is listed under no name, so that a card's name finds its own
                // face; its picture prints its source's name.
                let names = face
                    .plate
                    .as_ref()
                    .and_then(|plate| list.get(usize::from(plate.source.0)))
                    .map_or(&face.names, |source| &source.names);
                MatchFace {
                    scan,
                    key,
                    names: names.clone(),
                    colors: face.colors.clone(),
                    text: face.text.clone(),
                    plate: face.plate.clone(),
                }
            })
            .collect();
        Self { faces }
    }

    /// The face with this id.
    #[must_use]
    pub fn get(&self, face: FaceId) -> Option<&MatchFace> {
        self.faces.get(usize::from(face.0))
    }
}

/// The scan path and cache key of the scan at `image`, a URL that `card_records` made: a card
/// face's `https://cards.scryfall.io/large/<side>/<a>/<b>/<id>.jpg`, or the card back's
/// `https://backs.scryfall.io/large/<a>/<b>/<id>.jpg`.
fn scan_and_key(image: &str) -> (String, String) {
    let id = |path: &str| {
        let file = path.rsplit('/').next().unwrap_or(path);
        file.strip_suffix(".jpg").unwrap_or(file).to_owned()
    };
    if let Some(path) = image.strip_prefix(BACKS) {
        return (format!("/backs{path}"), format!("back-{}", id(path)));
    }
    let path = image.strip_prefix(IMAGES).unwrap_or(image);
    let side = path.split('/').nth(2).unwrap_or("front");
    (path.to_owned(), format!("{}-{side}", id(path)))
}

/// A 32-bit FNV-1a hash of `text`, which tells plate faces of one scan apart in the compressed
/// cache, so that a changed ability text is drawn again.
fn fnv(text: &str) -> u32 {
    text.bytes().fold(2_166_136_261_u32, |h, byte| {
        (h ^ u32::from(byte)).wrapping_mul(16_777_619)
    })
}
